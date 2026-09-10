/*
 * PFBinary v1 pure-C decoder for PHP 8.x.
 * Mirrors crates/pf-core decoder semantics (tags, varint, max_depth/size, STR_REF).
 * Hot path: stack zvals + interned zend_string table — no per-scalar FFI.
 */

#include <string.h>

#include "php.h"
#include "zend_hash.h"

#include "decode_c.h"

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

#define PF_MAGIC0 'P'
#define PF_MAGIC1 'F'
#define PF_MAGIC2 'B'
#define PF_MAGIC3 'N'
#define PF_VERSION_V1 0x01
#define PF_FLAGS_RESERVED 0xFCu

#define PF_DEFAULT_MAX_DEPTH 100u
#define PF_DEFAULT_MAX_SIZE  (10u * 1024u * 1024u)
#define PF_MAX_VARINT_LEN    10

typedef struct pf_cdec {
	const uint8_t *data;
	size_t len;
	size_t pos;
	uint32_t depth;
	uint32_t max_depth;
	size_t size_used;
	size_t max_size;
	int strdedup;
	zend_string **strs;
	uint32_t str_len;
	uint32_t str_cap;
} pf_cdec;

static void pf_cdec_free_strs(pf_cdec *d)
{
	uint32_t i;
	if (!d->strs) {
		return;
	}
	for (i = 0; i < d->str_len; i++) {
		if (d->strs[i]) {
			zend_string_release(d->strs[i]);
		}
	}
	efree(d->strs);
	d->strs = NULL;
	d->str_len = 0;
	d->str_cap = 0;
}

static pf_err pf_charge(pf_cdec *d, size_t n)
{
	if (n > d->max_size - d->size_used) {
		return PF_E_MAX_SIZE;
	}
	d->size_used += n;
	return PF_OK;
}

static pf_err pf_enter(pf_cdec *d)
{
	if (d->depth >= d->max_depth) {
		return PF_E_MAX_DEPTH;
	}
	d->depth++;
	return PF_OK;
}

static void pf_leave(pf_cdec *d)
{
	d->depth--;
}

static size_t pf_remaining(const pf_cdec *d)
{
	return d->len - d->pos;
}

static pf_err pf_take(pf_cdec *d, size_t n, const uint8_t **out)
{
	if (pf_remaining(d) < n) {
		return PF_E_TRUNCATED;
	}
	*out = d->data + d->pos;
	d->pos += n;
	return PF_OK;
}

static pf_err pf_u8(pf_cdec *d, uint8_t *out)
{
	const uint8_t *p;
	pf_err err = pf_take(d, 1, &p);
	if (err != PF_OK) {
		return err;
	}
	*out = *p;
	return PF_OK;
}

static pf_err pf_read_uvarint(pf_cdec *d, uint64_t *out)
{
	uint64_t result = 0;
	uint32_t shift = 0;
	size_t i;

	for (i = 0; i < PF_MAX_VARINT_LEN; i++) {
		uint8_t byte;
		uint64_t bits;
		pf_err err = pf_u8(d, &byte);
		if (err != PF_OK) {
			return err;
		}
		bits = (uint64_t)(byte & 0x7f);
		if (i == PF_MAX_VARINT_LEN - 1) {
			if (bits > 1 || (byte & 0x80) != 0) {
				return PF_E_VARINT;
			}
			result |= bits << 63;
			*out = result;
			return PF_OK;
		}
		result |= bits << shift;
		if ((byte & 0x80) == 0) {
			*out = result;
			return PF_OK;
		}
		shift += 7;
	}
	return PF_E_VARINT;
}

static pf_err pf_read_ivarint(pf_cdec *d, int64_t *out)
{
	uint64_t u;
	pf_err err = pf_read_uvarint(d, &u);
	if (err != PF_OK) {
		return err;
	}
	*out = (int64_t)(u >> 1) ^ (-(int64_t)(u & 1));
	return PF_OK;
}

static pf_err pf_check_count(pf_cdec *d, uint64_t count64, uint64_t min_bytes, uint32_t *out)
{
	uint64_t need;
	if (count64 > (uint64_t)UINT32_MAX) {
		return PF_E_OVERFLOW;
	}
	need = count64 * min_bytes;
	if (need > (uint64_t)pf_remaining(d)) {
		return PF_E_OVERFLOW;
	}
	*out = (uint32_t)count64;
	return PF_OK;
}

static pf_err pf_intern(pf_cdec *d, const uint8_t *bytes, size_t len, uint32_t *id_out)
{
	zend_string *zs;
	uint32_t id = d->str_len;
	if (d->str_len >= d->str_cap) {
		uint32_t ncap = d->str_cap ? d->str_cap * 2 : 16;
		d->strs = erealloc(d->strs, ncap * sizeof(zend_string *));
		d->str_cap = ncap;
	}
	zs = zend_string_init((const char *)bytes, len, 0);
	d->strs[d->str_len++] = zs;
	*id_out = id;
	return PF_OK;
}

static zend_string *pf_str_get(pf_cdec *d, uint32_t id)
{
	if (id >= d->str_len || !d->strs[id]) {
		return NULL;
	}
	return d->strs[id];
}

static void pf_set_i64(zval *out, int64_t v)
{
	if (v < (int64_t)ZEND_LONG_MIN || v > (int64_t)ZEND_LONG_MAX) {
		ZVAL_DOUBLE(out, (double)v);
	} else {
		ZVAL_LONG(out, (zend_long)v);
	}
}

static pf_err pf_decode_value(pf_cdec *d, zval *out);
static pf_err pf_decode_array_packed(pf_cdec *d, zval *out);
static pf_err pf_decode_array_hash(pf_cdec *d, zval *out);
static pf_err pf_decode_packed_longs(pf_cdec *d, zval *out);
static pf_err pf_decode_array_rows(pf_cdec *d, zval *out);
static pf_err pf_decode_array_repeat(pf_cdec *d, zval *out);
static pf_err pf_decode_value_tag(pf_cdec *d, uint8_t tag, zval *out);

static pf_err pf_decode_string_tag(pf_cdec *d, uint8_t tag, zval *out)
{
	if (tag == PF_TAG_STR_EMPTY) {
		ZVAL_EMPTY_STRING(out);
		return PF_OK;
	}
	if (tag == PF_TAG_STR_VARINT) {
		uint64_t len64;
		size_t len;
		const uint8_t *bytes;
		uint32_t id;
		pf_err err = pf_read_uvarint(d, &len64);
		if (err != PF_OK) {
			return err;
		}
		if (len64 > (uint64_t)pf_remaining(d)) {
			return PF_E_OVERFLOW;
		}
		len = (size_t)len64;
		err = pf_charge(d, len);
		if (err != PF_OK) {
			return err;
		}
		err = pf_take(d, len, &bytes);
		if (err != PF_OK) {
			return err;
		}
		err = pf_intern(d, bytes, len, &id);
		if (err != PF_OK) {
			return err;
		}
		ZVAL_STR_COPY(out, d->strs[id]);
		return PF_OK;
	}
	if (tag == PF_TAG_STR_REF) {
		uint64_t id64;
		uint32_t id;
		zend_string *zs;
		pf_err err;
		if (!d->strdedup) {
			return PF_E_UNKNOWN_TAG;
		}
		err = pf_read_uvarint(d, &id64);
		if (err != PF_OK) {
			return err;
		}
		if (id64 > (uint64_t)UINT32_MAX) {
			return PF_E_BAD_STRREF;
		}
		id = (uint32_t)id64;
		zs = pf_str_get(d, id);
		if (!zs) {
			return PF_E_BAD_STRREF;
		}
		err = pf_charge(d, ZSTR_LEN(zs));
		if (err != PF_OK) {
			return err;
		}
		ZVAL_STR_COPY(out, zs);
		return PF_OK;
	}
	return PF_E_UNKNOWN_TAG;
}

static pf_err pf_read_key(pf_cdec *d, zend_ulong *idx, zend_string **key_str, int *key_kind)
{
	/* key_kind: 0=int, 1=interned/shared zend_string*, 2=empty string */
	uint8_t tag;
	pf_err err = pf_u8(d, &tag);
	if (err != PF_OK) {
		return err;
	}
	*key_str = NULL;
	*key_kind = 0;
	switch (tag) {
		case PF_TAG_VARINT: {
			int64_t v;
			err = pf_read_ivarint(d, &v);
			if (err != PF_OK) {
				return err;
			}
			*idx = (zend_ulong)(zend_long)v;
			*key_kind = 0;
			return PF_OK;
		}
		case PF_TAG_STR_EMPTY:
			*key_kind = 2;
			return PF_OK;
		case PF_TAG_STR_VARINT: {
			uint64_t len64;
			size_t len;
			const uint8_t *bytes;
			uint32_t id;
			err = pf_read_uvarint(d, &len64);
			if (err != PF_OK) {
				return err;
			}
			if (len64 > (uint64_t)pf_remaining(d)) {
				return PF_E_OVERFLOW;
			}
			len = (size_t)len64;
			err = pf_charge(d, len);
			if (err != PF_OK) {
				return err;
			}
			err = pf_take(d, len, &bytes);
			if (err != PF_OK) {
				return err;
			}
			err = pf_intern(d, bytes, len, &id);
			if (err != PF_OK) {
				return err;
			}
			*key_str = d->strs[id];
			*key_kind = 1;
			return PF_OK;
		}
		case PF_TAG_STR_REF: {
			uint64_t id64;
			uint32_t id;
			zend_string *zs;
			if (!d->strdedup) {
				return PF_E_BAD_KEY;
			}
			err = pf_read_uvarint(d, &id64);
			if (err != PF_OK) {
				return err;
			}
			if (id64 > (uint64_t)UINT32_MAX) {
				return PF_E_BAD_STRREF;
			}
			id = (uint32_t)id64;
			zs = pf_str_get(d, id);
			if (!zs) {
				return PF_E_BAD_STRREF;
			}
			err = pf_charge(d, ZSTR_LEN(zs));
			if (err != PF_OK) {
				return err;
			}
			*key_str = zs;
			*key_kind = 1;
			return PF_OK;
		}
		default:
			return PF_E_BAD_KEY;
	}
}

static pf_err pf_decode_packed_longs(pf_cdec *d, zval *out)
{
	uint64_t count64;
	uint32_t count, i;
	pf_err err = pf_read_uvarint(d, &count64);
	if (err != PF_OK) {
		return err;
	}
	err = pf_check_count(d, count64, 1, &count);
	if (err != PF_OK) {
		return err;
	}
	err = pf_enter(d);
	if (err != PF_OK) {
		return err;
	}
	array_init_size(out, count);
	if (count > 0) {
		zend_hash_real_init_packed(Z_ARRVAL_P(out));
	}
	for (i = 0; i < count; i++) {
		int64_t v;
		zval tmp;
		err = pf_read_ivarint(d, &v);
		if (err != PF_OK) {
			zval_ptr_dtor(out);
			ZVAL_UNDEF(out);
			pf_leave(d);
			return err;
		}
		pf_set_i64(&tmp, v);
		if (zend_hash_next_index_insert(Z_ARRVAL_P(out), &tmp) == NULL) {
			zval_ptr_dtor(&tmp);
			zval_ptr_dtor(out);
			ZVAL_UNDEF(out);
			pf_leave(d);
			return PF_E_HOST;
		}
	}
	pf_leave(d);
	return PF_OK;
}

static pf_err pf_decode_array_packed(pf_cdec *d, zval *out)
{
	uint64_t count64;
	uint32_t count, i;
	pf_err err = pf_read_uvarint(d, &count64);
	if (err != PF_OK) {
		return err;
	}
	err = pf_check_count(d, count64, 1, &count);
	if (err != PF_OK) {
		return err;
	}
	err = pf_enter(d);
	if (err != PF_OK) {
		return err;
	}
	array_init_size(out, count);
	if (count > 0) {
		zend_hash_real_init_packed(Z_ARRVAL_P(out));
	}
	for (i = 0; i < count; i++) {
		zval tmp;
		ZVAL_UNDEF(&tmp);
		err = pf_decode_value(d, &tmp);
		if (err != PF_OK) {
			zval_ptr_dtor(out);
			ZVAL_UNDEF(out);
			pf_leave(d);
			return err;
		}
		if (zend_hash_next_index_insert(Z_ARRVAL_P(out), &tmp) == NULL) {
			zval_ptr_dtor(&tmp);
			zval_ptr_dtor(out);
			ZVAL_UNDEF(out);
			pf_leave(d);
			return PF_E_HOST;
		}
	}
	pf_leave(d);
	return PF_OK;
}

static pf_err pf_decode_array_hash(pf_cdec *d, zval *out)
{
	uint64_t count64;
	uint32_t count, i;
	pf_err err = pf_read_uvarint(d, &count64);
	if (err != PF_OK) {
		return err;
	}
	err = pf_check_count(d, count64, 2, &count);
	if (err != PF_OK) {
		return err;
	}
	err = pf_enter(d);
	if (err != PF_OK) {
		return err;
	}
	array_init_size(out, count);
	for (i = 0; i < count; i++) {
		zend_ulong idx = 0;
		zend_string *key_str = NULL;
		int key_kind = 0;
		zval tmp;
		ZVAL_UNDEF(&tmp);
		err = pf_read_key(d, &idx, &key_str, &key_kind);
		if (err != PF_OK) {
			zval_ptr_dtor(out);
			ZVAL_UNDEF(out);
			pf_leave(d);
			return err;
		}
		err = pf_decode_value(d, &tmp);
		if (err != PF_OK) {
			zval_ptr_dtor(out);
			ZVAL_UNDEF(out);
			pf_leave(d);
			return err;
		}
		if (key_kind == 1) {
			if (zend_symtable_update(Z_ARRVAL_P(out), key_str, &tmp) == NULL) {
				zval_ptr_dtor(&tmp);
				zval_ptr_dtor(out);
				ZVAL_UNDEF(out);
				pf_leave(d);
				return PF_E_HOST;
			}
		} else if (key_kind == 2) {
			if (zend_symtable_str_update(Z_ARRVAL_P(out), "", 0, &tmp) == NULL) {
				zval_ptr_dtor(&tmp);
				zval_ptr_dtor(out);
				ZVAL_UNDEF(out);
				pf_leave(d);
				return PF_E_HOST;
			}
		} else {
			if (zend_hash_index_update(Z_ARRVAL_P(out), idx, &tmp) == NULL) {
				zval_ptr_dtor(&tmp);
				zval_ptr_dtor(out);
				ZVAL_UNDEF(out);
				pf_leave(d);
				return PF_E_HOST;
			}
		}
	}
	pf_leave(d);
	return PF_OK;
}

static pf_err pf_decode_array_rows(pf_cdec *d, zval *out)
{
	uint64_t nrows64, ncols64;
	uint32_t nrows, ncols, r, c;
	typedef struct {
		int kind;
		zend_ulong idx;
		zend_string *str;
	} pf_row_key;
	pf_row_key *keys;
	pf_err err;

	err = pf_read_uvarint(d, &nrows64);
	if (err != PF_OK) {
		return err;
	}
	err = pf_read_uvarint(d, &ncols64);
	if (err != PF_OK) {
		return err;
	}
	if (nrows64 > (uint64_t)UINT32_MAX || ncols64 > (uint64_t)UINT32_MAX) {
		return PF_E_OVERFLOW;
	}
	nrows = (uint32_t)nrows64;
	ncols = (uint32_t)ncols64;
	{
		uint64_t need = ncols64 + nrows64 * (ncols64 > 0 ? ncols64 : 1);
		if (need > (uint64_t)pf_remaining(d)) {
			return PF_E_OVERFLOW;
		}
	}

	err = pf_enter(d);
	if (err != PF_OK) {
		return err;
	}

	keys = ecalloc(ncols ? ncols : 1, sizeof(pf_row_key));
	for (c = 0; c < ncols; c++) {
		err = pf_read_key(d, &keys[c].idx, &keys[c].str, &keys[c].kind);
		if (err != PF_OK) {
			efree(keys);
			pf_leave(d);
			return err;
		}
	}

	array_init_size(out, nrows);
	if (nrows > 0) {
		zend_hash_real_init_packed(Z_ARRVAL_P(out));
	}
	for (r = 0; r < nrows; r++) {
		zval row;
		ZVAL_UNDEF(&row);
		err = pf_enter(d);
		if (err != PF_OK) {
			zval_ptr_dtor(out);
			ZVAL_UNDEF(out);
			efree(keys);
			pf_leave(d);
			return err;
		}
		array_init_size(&row, ncols);
		for (c = 0; c < ncols; c++) {
			zval cell;
			ZVAL_UNDEF(&cell);
			err = pf_decode_value(d, &cell);
			if (err != PF_OK) {
				zval_ptr_dtor(&row);
				zval_ptr_dtor(out);
				ZVAL_UNDEF(out);
				efree(keys);
				pf_leave(d);
				pf_leave(d);
				return err;
			}
			if (keys[c].kind == 1) {
				if (zend_symtable_update(Z_ARRVAL(row), keys[c].str, &cell) == NULL) {
					zval_ptr_dtor(&cell);
					zval_ptr_dtor(&row);
					zval_ptr_dtor(out);
					ZVAL_UNDEF(out);
					efree(keys);
					pf_leave(d);
					pf_leave(d);
					return PF_E_HOST;
				}
			} else if (keys[c].kind == 2) {
				if (zend_symtable_str_update(Z_ARRVAL(row), "", 0, &cell) == NULL) {
					zval_ptr_dtor(&cell);
					zval_ptr_dtor(&row);
					zval_ptr_dtor(out);
					ZVAL_UNDEF(out);
					efree(keys);
					pf_leave(d);
					pf_leave(d);
					return PF_E_HOST;
				}
			} else {
				if (zend_hash_index_update(Z_ARRVAL(row), keys[c].idx, &cell) == NULL) {
					zval_ptr_dtor(&cell);
					zval_ptr_dtor(&row);
					zval_ptr_dtor(out);
					ZVAL_UNDEF(out);
					efree(keys);
					pf_leave(d);
					pf_leave(d);
					return PF_E_HOST;
				}
			}
		}
		pf_leave(d);
		if (zend_hash_next_index_insert(Z_ARRVAL_P(out), &row) == NULL) {
			zval_ptr_dtor(&row);
			zval_ptr_dtor(out);
			ZVAL_UNDEF(out);
			efree(keys);
			pf_leave(d);
			return PF_E_HOST;
		}
	}
	efree(keys);
	pf_leave(d);
	return PF_OK;
}

static pf_err pf_decode_array_repeat(pf_cdec *d, zval *out)
{
	uint64_t count64;
	uint32_t count, i;
	zval template;
	pf_err err;

	err = pf_read_uvarint(d, &count64);
	if (err != PF_OK) {
		return err;
	}
	if (count64 > (uint64_t)UINT32_MAX) {
		return PF_E_OVERFLOW;
	}
	count = (uint32_t)count64;
	/* Single template value follows — do not require count bytes remaining. */
	if (count > 0 && pf_remaining(d) < 1) {
		return PF_E_TRUNCATED;
	}
	err = pf_enter(d);
	if (err != PF_OK) {
		return err;
	}
	ZVAL_UNDEF(&template);
	err = pf_decode_value(d, &template);
	if (err != PF_OK) {
		pf_leave(d);
		return err;
	}
	array_init_size(out, count);
	if (count > 0) {
		zend_hash_real_init_packed(Z_ARRVAL_P(out));
	}
	for (i = 0; i < count; i++) {
		zval copy;
		ZVAL_COPY(&copy, &template);
		if (zend_hash_next_index_insert(Z_ARRVAL_P(out), &copy) == NULL) {
			zval_ptr_dtor(&copy);
			zval_ptr_dtor(&template);
			zval_ptr_dtor(out);
			ZVAL_UNDEF(out);
			pf_leave(d);
			return PF_E_HOST;
		}
	}
	zval_ptr_dtor(&template);
	pf_leave(d);
	return PF_OK;
}

static pf_err pf_decode_value_tag(pf_cdec *d, uint8_t tag, zval *out)
{
	pf_err err;
	switch (tag) {
		case PF_TAG_NULL:
			ZVAL_NULL(out);
			return PF_OK;
		case PF_TAG_FALSE:
			ZVAL_FALSE(out);
			return PF_OK;
		case PF_TAG_TRUE:
			ZVAL_TRUE(out);
			return PF_OK;
		case PF_TAG_VARINT: {
			int64_t v;
			err = pf_read_ivarint(d, &v);
			if (err != PF_OK) {
				return err;
			}
			pf_set_i64(out, v);
			return PF_OK;
		}
		case PF_TAG_DOUBLE: {
			const uint8_t *raw;
			uint64_t bits = 0;
			double f;
			size_t i;
			err = pf_take(d, 8, &raw);
			if (err != PF_OK) {
				return err;
			}
			for (i = 0; i < 8; i++) {
				bits |= ((uint64_t)raw[i]) << (8 * i);
			}
			memcpy(&f, &bits, sizeof(f));
			ZVAL_DOUBLE(out, f);
			return PF_OK;
		}
		case PF_TAG_STR_EMPTY:
		case PF_TAG_STR_VARINT:
		case PF_TAG_STR_REF:
			return pf_decode_string_tag(d, tag, out);
		case PF_TAG_ARRAY_PACKED:
			return pf_decode_array_packed(d, out);
		case PF_TAG_ARRAY_HASH:
			return pf_decode_array_hash(d, out);
		case PF_TAG_PACKED_LONGS:
			return pf_decode_packed_longs(d, out);
		case PF_TAG_ARRAY_ROWS:
			return pf_decode_array_rows(d, out);
		case PF_TAG_ARRAY_REPEAT:
			return pf_decode_array_repeat(d, out);
		default:
			return PF_E_UNKNOWN_TAG;
	}
}

static pf_err pf_decode_value(pf_cdec *d, zval *out)
{
	uint8_t tag;
	pf_err err = pf_u8(d, &tag);
	if (err != PF_OK) {
		return err;
	}
	return pf_decode_value_tag(d, tag, out);
}

pf_err pf_c_decode(
	const uint8_t *data,
	size_t len,
	uint32_t max_depth,
	size_t max_size,
	zval *out
)
{
	pf_cdec d;
	uint8_t version, flags;
	pf_err err;

	ZVAL_UNDEF(out);

	if (!data || !out) {
		return PF_E_NULL_ARG;
	}
	if (len < 6) {
		return PF_E_TRUNCATED;
	}
	if (data[0] != PF_MAGIC0 || data[1] != PF_MAGIC1
		|| data[2] != PF_MAGIC2 || data[3] != PF_MAGIC3) {
		return PF_E_BAD_MAGIC;
	}
	version = data[4];
	if (version != PF_VERSION_V1) {
		return PF_E_UNSUPPORTED_VERSION;
	}
	flags = data[5];
	if (flags & PF_FLAGS_RESERVED) {
		return PF_E_UNKNOWN_FLAGS;
	}
	if (flags & PF_FLAG_CHECKSUM) {
		/* V1 PHP path: checksum unsupported (flag reserved for reject). */
		return PF_E_CHECKSUM;
	}

	memset(&d, 0, sizeof(d));
	d.data = data + 6;
	d.len = len - 6;
	d.pos = 0;
	d.depth = 0;
	d.max_depth = max_depth ? max_depth : PF_DEFAULT_MAX_DEPTH;
	d.max_size = max_size ? max_size : PF_DEFAULT_MAX_SIZE;
	d.strdedup = (flags & PF_FLAG_STRDEDUP) != 0;

	if (d.len > d.max_size) {
		return PF_E_MAX_SIZE;
	}

	err = pf_decode_value(&d, out);
	if (err != PF_OK) {
		zval_ptr_dtor(out);
		ZVAL_UNDEF(out);
		pf_cdec_free_strs(&d);
		return err;
	}
	if (d.pos != d.len) {
		zval_ptr_dtor(out);
		ZVAL_UNDEF(out);
		pf_cdec_free_strs(&d);
		return PF_E_TRAILING;
	}
	pf_cdec_free_strs(&d);
	return PF_OK;
}
