/*
 * PFBinary v1 pure-C encoder for PHP 8.x.
 * Tuned for encode ≤ serialize and list/row workloads vs igbinary.
 */

#include <string.h>

#include "php.h"
#include "zend_hash.h"

#include "encode_c.h"

#define PF_TAG_NULL         0x00
#define PF_TAG_FALSE        0x01
#define PF_TAG_TRUE         0x02
#define PF_TAG_VARINT       0x03
#define PF_TAG_DOUBLE       0x04
#define PF_TAG_STR_EMPTY    0x05
#define PF_TAG_STR_VARINT   0x06
#define PF_TAG_STR_REF      0x07
#define PF_TAG_ARRAY_PACKED 0x08
#define PF_TAG_ARRAY_HASH   0x09
#define PF_TAG_PACKED_LONGS 0x0A
#define PF_TAG_ARRAY_ROWS   0x0B
#define PF_TAG_ARRAY_REPEAT 0x0C

#define PF_ENCODE_MAX_DEPTH 100
#define PF_HEADER_LEN       6
#define PF_INIT_CAP         512

typedef struct pf_cenc {
	uint8_t *buf;
	size_t len;
	size_t cap;
	HashTable strtab;
	int strtab_ready;
	uint32_t next_str_id;
	int strdedup;
	int depth;
	int max_depth;
} pf_cenc;

static zend_always_inline pf_err enc_reserve(pf_cenc *e, size_t n)
{
	size_t need = e->len + n;
	size_t ncap;
	if (need <= e->cap) {
		return PF_OK;
	}
	ncap = e->cap ? e->cap * 2 : PF_INIT_CAP;
	while (ncap < need) {
		ncap *= 2;
	}
	e->buf = erealloc(e->buf, ncap);
	e->cap = ncap;
	return PF_OK;
}

static zend_always_inline pf_err enc_u8(pf_cenc *e, uint8_t v)
{
	if (UNEXPECTED(e->len >= e->cap)) {
		if (enc_reserve(e, 1) != PF_OK) {
			return PF_E_INTERNAL;
		}
	}
	e->buf[e->len++] = v;
	return PF_OK;
}

static zend_always_inline pf_err enc_bytes(pf_cenc *e, const uint8_t *p, size_t n)
{
	if (enc_reserve(e, n) != PF_OK) {
		return PF_E_INTERNAL;
	}
	memcpy(e->buf + e->len, p, n);
	e->len += n;
	return PF_OK;
}

static zend_always_inline pf_err enc_uvarint(pf_cenc *e, uint64_t value)
{
	uint8_t tmp[10];
	size_t i = 0;
	do {
		uint8_t byte = (uint8_t)(value & 0x7f);
		value >>= 7;
		if (value != 0) {
			byte |= 0x80;
		}
		tmp[i++] = byte;
	} while (value != 0);
	return enc_bytes(e, tmp, i);
}

static zend_always_inline pf_err enc_ivarint(pf_cenc *e, int64_t n)
{
	uint64_t zz = ((uint64_t)n << 1) ^ (uint64_t)(n >> 63);
	return enc_uvarint(e, zz);
}

static zend_always_inline void enc_ensure_strtab(pf_cenc *e)
{
	if (!e->strtab_ready) {
		zend_hash_init(&e->strtab, 8, NULL, NULL, 0);
		e->strtab_ready = 1;
	}
}

static pf_err enc_zstr(pf_cenc *e, zend_string *zs)
{
	size_t len = ZSTR_LEN(zs);
	pf_err err;

	if (len == 0) {
		return enc_u8(e, PF_TAG_STR_EMPTY);
	}
	if (e->strdedup) {
		zval *found;
		enc_ensure_strtab(e);
		found = zend_hash_find(&e->strtab, zs);
		if (found) {
			err = enc_u8(e, PF_TAG_STR_REF);
			if (err != PF_OK) {
				return err;
			}
			return enc_uvarint(e, (uint64_t)(zend_ulong)Z_LVAL_P(found));
		}
		{
			zval id;
			ZVAL_LONG(&id, (zend_long)e->next_str_id);
			zend_hash_add_new(&e->strtab, zs, &id);
			e->next_str_id++;
		}
	}
	err = enc_u8(e, PF_TAG_STR_VARINT);
	if (err != PF_OK) {
		return err;
	}
	err = enc_uvarint(e, (uint64_t)len);
	if (err != PF_OK) {
		return err;
	}
	return enc_bytes(e, (const uint8_t *)ZSTR_VAL(zs), len);
}

static pf_err enc_string(pf_cenc *e, const char *s, size_t len)
{
	pf_err err;
	if (len == 0) {
		return enc_u8(e, PF_TAG_STR_EMPTY);
	}
	if (e->strdedup) {
		zval *found;
		enc_ensure_strtab(e);
		found = zend_hash_str_find(&e->strtab, s, len);
		if (found) {
			err = enc_u8(e, PF_TAG_STR_REF);
			if (err != PF_OK) {
				return err;
			}
			return enc_uvarint(e, (uint64_t)(zend_ulong)Z_LVAL_P(found));
		}
		{
			zval id;
			ZVAL_LONG(&id, (zend_long)e->next_str_id);
			zend_hash_str_update(&e->strtab, s, len, &id);
			e->next_str_id++;
		}
	}
	err = enc_u8(e, PF_TAG_STR_VARINT);
	if (err != PF_OK) {
		return err;
	}
	err = enc_uvarint(e, (uint64_t)len);
	if (err != PF_OK) {
		return err;
	}
	return enc_bytes(e, (const uint8_t *)s, len);
}

static int array_is_packed_longs(HashTable *ht)
{
	zval *val;
	ZEND_HASH_FOREACH_VAL(ht, val) {
		ZVAL_DEINDIRECT(val);
		ZVAL_DEREF(val);
		if (Z_TYPE_P(val) != IS_LONG) {
			return 0;
		}
	} ZEND_HASH_FOREACH_END();
	return 1;
}

/* Key-order schema compare; prefers interned zend_string* identity. */
static int schemas_match(HashTable *a, HashTable *b)
{
	uint32_t i = 0, j = 0;

	if (zend_hash_num_elements(a) != zend_hash_num_elements(b)) {
		return 0;
	}
	while (i < a->nNumUsed && j < b->nNumUsed) {
		Bucket *ba, *bb;
		while (i < a->nNumUsed && Z_TYPE(a->arData[i].val) == IS_UNDEF) {
			i++;
		}
		while (j < b->nNumUsed && Z_TYPE(b->arData[j].val) == IS_UNDEF) {
			j++;
		}
		if (i >= a->nNumUsed || j >= b->nNumUsed) {
			break;
		}
		ba = &a->arData[i];
		bb = &b->arData[j];
		if (ba->key) {
			if (!bb->key) {
				return 0;
			}
			if (ba->key != bb->key
				&& (ZSTR_LEN(ba->key) != ZSTR_LEN(bb->key)
					|| memcmp(ZSTR_VAL(ba->key), ZSTR_VAL(bb->key), ZSTR_LEN(ba->key)) != 0)) {
				return 0;
			}
		} else if (bb->key || ba->h != bb->h) {
			return 0;
		}
		i++;
		j++;
	}
	while (i < a->nNumUsed && Z_TYPE(a->arData[i].val) == IS_UNDEF) {
		i++;
	}
	while (j < b->nNumUsed && Z_TYPE(b->arData[j].val) == IS_UNDEF) {
		j++;
	}
	return i >= a->nNumUsed && j >= b->nNumUsed;
}

static int array_is_repeat(HashTable *ht, uint32_t *count_out)
{
	uint32_t n = zend_hash_num_elements(ht);
	zval *first;
	zval *val;

	if (n < 2 || !zend_array_is_list(ht)) {
		return 0;
	}
	first = zend_hash_index_find(ht, 0);
	if (!first) {
		return 0;
	}
	ZVAL_DEINDIRECT(first);
	ZVAL_DEREF(first);
	if (Z_TYPE_P(first) == IS_ARRAY) {
		HashTable *fa = Z_ARRVAL_P(first);
		ZEND_HASH_FOREACH_VAL(ht, val) {
			ZVAL_DEINDIRECT(val);
			ZVAL_DEREF(val);
			if (Z_TYPE_P(val) != IS_ARRAY || Z_ARRVAL_P(val) != fa) {
				if (!zend_is_identical(first, val)) {
					return 0;
				}
			}
		} ZEND_HASH_FOREACH_END();
	} else {
		ZEND_HASH_FOREACH_VAL(ht, val) {
			ZVAL_DEINDIRECT(val);
			ZVAL_DEREF(val);
			if (!zend_is_identical(first, val)) {
				return 0;
			}
		} ZEND_HASH_FOREACH_END();
	}
	*count_out = n;
	return 1;
}

static int array_is_rowset(HashTable *ht, uint32_t *nrows_out, uint32_t *ncols_out)
{
	uint32_t n = zend_hash_num_elements(ht);
	zval *first;
	HashTable *schema;
	zval *val;
	uint32_t ncols;

	if (n < 2 || !zend_array_is_list(ht)) {
		return 0;
	}
	first = zend_hash_index_find(ht, 0);
	if (!first) {
		return 0;
	}
	ZVAL_DEINDIRECT(first);
	ZVAL_DEREF(first);
	if (Z_TYPE_P(first) != IS_ARRAY) {
		return 0;
	}
	schema = Z_ARRVAL_P(first);
	ncols = zend_hash_num_elements(schema);
	if (ncols == 0) {
		return 0;
	}
	ZEND_HASH_FOREACH_VAL(ht, val) {
		ZVAL_DEINDIRECT(val);
		ZVAL_DEREF(val);
		if (Z_TYPE_P(val) != IS_ARRAY) {
			return 0;
		}
		if (!schemas_match(schema, Z_ARRVAL_P(val))) {
			return 0;
		}
	} ZEND_HASH_FOREACH_END();
	*nrows_out = n;
	*ncols_out = ncols;
	return 1;
}

static pf_err enc_zval(pf_cenc *e, zval *zv);

static pf_err enc_key(pf_cenc *e, zend_string *key, zend_ulong idx)
{
	if (key) {
		return enc_zstr(e, key);
	}
	{
		pf_err err = enc_u8(e, PF_TAG_VARINT);
		if (err != PF_OK) {
			return err;
		}
		return enc_ivarint(e, (int64_t)(zend_long)idx);
	}
}

static pf_err enc_array(pf_cenc *e, zval *zv)
{
	HashTable *ht = Z_ARRVAL_P(zv);
	uint32_t count = zend_hash_num_elements(ht);
	uint32_t nrows, ncols;
	pf_err err;
	int is_list;

	is_list = zend_array_is_list(ht) ? 1 : 0;

	if (is_list && count > 0 && array_is_packed_longs(ht)) {
		err = enc_u8(e, PF_TAG_PACKED_LONGS);
		if (err != PF_OK) {
			return err;
		}
		err = enc_uvarint(e, count);
		if (err != PF_OK) {
			return err;
		}
		ZEND_HASH_FOREACH_VAL(ht, zval *val) {
			ZVAL_DEINDIRECT(val);
			ZVAL_DEREF(val);
			err = enc_ivarint(e, (int64_t)Z_LVAL_P(val));
			if (err != PF_OK) {
				return err;
			}
		} ZEND_HASH_FOREACH_END();
		return PF_OK;
	}

	if (is_list && count >= 2) {
		zval *first = zend_hash_index_find(ht, 0);
		if (first) {
			ZVAL_DEINDIRECT(first);
			ZVAL_DEREF(first);
			if (Z_TYPE_P(first) == IS_ARRAY) {
				zval *second = zend_hash_index_find(ht, 1);
				int maybe_repeat = 0;
				/*
				 * Avoid full REPEAT scan when row[1] already differs — common
				 * for feed/PLP (same schema, different values).
				 */
				if (second) {
					ZVAL_DEINDIRECT(second);
					ZVAL_DEREF(second);
					maybe_repeat = zend_is_identical(first, second);
				}
				if (maybe_repeat && array_is_repeat(ht, &nrows)) {
					err = enc_u8(e, PF_TAG_ARRAY_REPEAT);
					if (err != PF_OK) {
						return err;
					}
					err = enc_uvarint(e, nrows);
					if (err != PF_OK) {
						return err;
					}
					return enc_zval(e, first);
				}
				if (array_is_rowset(ht, &nrows, &ncols)) {
					zend_ulong idx;
					zend_string *key;
					zval *val;
					err = enc_u8(e, PF_TAG_ARRAY_ROWS);
					if (err != PF_OK) {
						return err;
					}
					err = enc_uvarint(e, nrows);
					if (err != PF_OK) {
						return err;
					}
					err = enc_uvarint(e, ncols);
					if (err != PF_OK) {
						return err;
					}
					ZEND_HASH_FOREACH_KEY(Z_ARRVAL_P(first), idx, key) {
						err = enc_key(e, key, idx);
						if (err != PF_OK) {
							return err;
						}
					} ZEND_HASH_FOREACH_END();
					ZEND_HASH_FOREACH_VAL(ht, val) {
						HashTable *row;
						ZVAL_DEINDIRECT(val);
						ZVAL_DEREF(val);
						row = Z_ARRVAL_P(val);
						ZEND_HASH_FOREACH_VAL(row, zval *cell) {
							ZVAL_DEINDIRECT(cell);
							err = enc_zval(e, cell);
							if (err != PF_OK) {
								return err;
							}
						} ZEND_HASH_FOREACH_END();
					} ZEND_HASH_FOREACH_END();
					return PF_OK;
				}
			} else if (array_is_repeat(ht, &nrows)) {
				err = enc_u8(e, PF_TAG_ARRAY_REPEAT);
				if (err != PF_OK) {
					return err;
				}
				err = enc_uvarint(e, nrows);
				if (err != PF_OK) {
					return err;
				}
				return enc_zval(e, first);
			}
		}
	}

	if (is_list) {
		err = enc_u8(e, PF_TAG_ARRAY_PACKED);
		if (err != PF_OK) {
			return err;
		}
		err = enc_uvarint(e, count);
		if (err != PF_OK) {
			return err;
		}
		ZEND_HASH_FOREACH_VAL(ht, zval *val) {
			ZVAL_DEINDIRECT(val);
			err = enc_zval(e, val);
			if (err != PF_OK) {
				return err;
			}
		} ZEND_HASH_FOREACH_END();
		return PF_OK;
	}

	err = enc_u8(e, PF_TAG_ARRAY_HASH);
	if (err != PF_OK) {
		return err;
	}
	err = enc_uvarint(e, count);
	if (err != PF_OK) {
		return err;
	}
	{
		zend_ulong idx;
		zend_string *key;
		zval *val;
		ZEND_HASH_FOREACH_KEY_VAL(ht, idx, key, val) {
			err = enc_key(e, key, idx);
			if (err != PF_OK) {
				return err;
			}
			ZVAL_DEINDIRECT(val);
			err = enc_zval(e, val);
			if (err != PF_OK) {
				return err;
			}
		} ZEND_HASH_FOREACH_END();
	}
	return PF_OK;
}

static pf_err enc_zval(pf_cenc *e, zval *zv)
{
	if (e->depth > e->max_depth) {
		return PF_E_MAX_DEPTH;
	}
	ZVAL_DEREF(zv);
	switch (Z_TYPE_P(zv)) {
		case IS_NULL:
			return enc_u8(e, PF_TAG_NULL);
		case IS_FALSE:
			return enc_u8(e, PF_TAG_FALSE);
		case IS_TRUE:
			return enc_u8(e, PF_TAG_TRUE);
		case IS_LONG: {
			pf_err err = enc_u8(e, PF_TAG_VARINT);
			if (err != PF_OK) {
				return err;
			}
			return enc_ivarint(e, (int64_t)Z_LVAL_P(zv));
		}
		case IS_DOUBLE: {
			uint64_t bits;
			uint8_t le[8];
			size_t i;
			pf_err err = enc_u8(e, PF_TAG_DOUBLE);
			if (err != PF_OK) {
				return err;
			}
			memcpy(&bits, &Z_DVAL_P(zv), sizeof(bits));
			for (i = 0; i < 8; i++) {
				le[i] = (uint8_t)((bits >> (8 * i)) & 0xff);
			}
			return enc_bytes(e, le, 8);
		}
		case IS_STRING:
			return enc_zstr(e, Z_STR_P(zv));
		case IS_ARRAY: {
			pf_err err;
			e->depth++;
			err = enc_array(e, zv);
			e->depth--;
			return err;
		}
		case IS_INDIRECT:
			return enc_zval(e, Z_INDIRECT_P(zv));
		case IS_OBJECT:
		case IS_RESOURCE:
		default:
			return PF_E_UNSUPPORTED_TYPE;
	}
}

pf_err pf_c_encode(zval *zv, uint8_t flags, uint8_t **out, size_t *out_len)
{
	pf_cenc e;
	pf_err err;

	if (!zv || !out || !out_len) {
		return PF_E_NULL_ARG;
	}
	if (flags & ~((uint8_t)(PF_FLAG_CHECKSUM | PF_FLAG_STRDEDUP))) {
		return PF_E_UNKNOWN_FLAGS;
	}
	if (flags & PF_FLAG_CHECKSUM) {
		return PF_E_CHECKSUM;
	}

	memset(&e, 0, sizeof(e));
	e.buf = emalloc(PF_INIT_CAP);
	e.cap = PF_INIT_CAP;
	e.len = PF_HEADER_LEN; /* single buffer: reserve header, no final memcpy */
	e.strdedup = (flags & PF_FLAG_STRDEDUP) != 0;
	e.max_depth = PF_ENCODE_MAX_DEPTH;

	err = enc_zval(&e, zv);
	if (err != PF_OK) {
		if (e.strtab_ready) {
			zend_hash_destroy(&e.strtab);
		}
		efree(e.buf);
		return err;
	}

	e.buf[0] = 'P';
	e.buf[1] = 'F';
	e.buf[2] = 'B';
	e.buf[3] = 'N';
	e.buf[4] = 0x01;
	e.buf[5] = flags;

	if (e.strtab_ready) {
		zend_hash_destroy(&e.strtab);
	}

	*out = e.buf;
	*out_len = e.len;
	return PF_OK;
}
