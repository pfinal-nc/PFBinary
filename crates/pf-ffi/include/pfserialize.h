/* PFSerialize C ABI — PFBinary v1
 *
 * Header for php/ and native callers. Link against libpfserialize.
 */

#ifndef PFSERIALIZE_H
#define PFSERIALIZE_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef enum pf_err {
    PF_OK = 0,
    PF_E_BAD_MAGIC = 1,
    PF_E_UNSUPPORTED_VERSION = 2,
    PF_E_UNKNOWN_FLAGS = 3,
    PF_E_CHECKSUM = 4,
    PF_E_TRUNCATED = 5,
    PF_E_TRAILING = 6,
    PF_E_UNKNOWN_TAG = 7,
    PF_E_BAD_KEY = 8,
    PF_E_VARINT = 9,
    PF_E_BAD_STRREF = 10,
    PF_E_MAX_DEPTH = 11,
    PF_E_MAX_SIZE = 12,
    PF_E_OVERFLOW = 13,
    PF_E_UNSUPPORTED_TYPE = 14,
    PF_E_HOST = 15,
    PF_E_NULL_ARG = 16,
    PF_E_INTERNAL = 127
} pf_err;

/* Flags (wire header) */
#define PF_FLAG_CHECKSUM  0x01u
#define PF_FLAG_STRDEDUP  0x02u
#define PF_FLAGS_DEFAULT  PF_FLAG_STRDEDUP

/* ---------- buffer ---------- */

void pf_free(uint8_t *ptr);

/* ---------- decode host vtable ---------- */

/**
 * Scalar kinds for direct array writes (production hot path).
 * Avoids per-scalar heap zval allocation in PHP.
 */
typedef enum pf_scalar_kind {
    PF_SCALAR_NULL = 0,
    PF_SCALAR_BOOL = 1,
    PF_SCALAR_I64 = 2,
    PF_SCALAR_F64 = 3,
    PF_SCALAR_STRING = 4,
    PF_SCALAR_STRING_ID = 5
} pf_scalar_kind;

typedef struct pf_scalar {
    uint8_t kind;
    int bool_val;
    int64_t i64_val;
    double f64_val;
    const uint8_t *str;
    size_t str_len;
    uint32_t str_id;
} pf_scalar;

/**
 * Host callbacks for decode. Opaque handles are owned by the host.
 * On failure mid-array, ArrayState/Value must free themselves via Drop/destructor
 * semantics on the C side (e.g. zval_ptr_dtor when the pointer is released).
 *
 * All callbacks return 0 on success, non-zero on failure (mapped to PF_E_HOST).
 *
 * Production: implement push_scalar / insert_scalar_* to write stack zvals.
 * Implement intern_string + *_string_key_id / STRING_ID to reuse zend_string on STR_REF.
 * Nested arrays still use begin_array / push_packed / insert_*_key / end_array.
 */
typedef struct pf_host_vtable {
    int (*make_null)(void *ctx, void **out_value);
    int (*make_bool)(void *ctx, int v, void **out_value);
    int (*make_i64)(void *ctx, int64_t v, void **out_value);
    int (*make_f64)(void *ctx, double v, void **out_value);
    int (*make_string)(void *ctx, const uint8_t *bytes, size_t len, void **out_value);

    int (*begin_array)(void *ctx, int packed, uint32_t count, void **out_array);
    int (*push_packed)(void *ctx, void *array, void *value);
    int (*insert_i64_key)(void *ctx, void *array, int64_t key, void *value);
    int (*insert_string_key)(void *ctx, void *array, const uint8_t *key, size_t key_len, void *value);
    int (*end_array)(void *ctx, void *array, void **out_value);

    /** Optional: release a value that will not be used (may be NULL). */
    void (*drop_value)(void *ctx, void *value);
    /** Optional: release an array state that will not be finished (may be NULL). */
    void (*drop_array)(void *ctx, void *array);

    /** Optional production hot path (may be NULL → fallback to make_* + push/insert). */
    int (*push_scalar)(void *ctx, void *array, const pf_scalar *scalar);
    int (*insert_scalar_i64_key)(void *ctx, void *array, int64_t key, const pf_scalar *scalar);
    int (*insert_scalar_string_key)(void *ctx, void *array, const uint8_t *key, size_t key_len, const pf_scalar *scalar);
    /** Optional: bulk append packed int64 run (one call for N integers). */
    int (*push_i64_run)(void *ctx, void *array, const int64_t *values, size_t count);

    /** Optional: first-definition string table → host-native interned string. */
    int (*intern_string)(void *ctx, uint32_t id, const uint8_t *bytes, size_t len);
    int (*make_string_id)(void *ctx, uint32_t id, void **out_value);
    int (*insert_string_key_id)(void *ctx, void *array, uint32_t key_id, void *value);
    int (*insert_scalar_string_key_id)(void *ctx, void *array, uint32_t key_id, const pf_scalar *scalar);
    /** Optional: deep-copy a host value (ARRAY_REPEAT). */
    int (*duplicate_value)(void *ctx, void *value, void **out_value);
} pf_host_vtable;

/**
 * Decode `data[0..len)` into a host value.
 * On success, *out_value is set by the host (via end callbacks / make_*).
 * max_depth/max_size: 0 means defaults (100 / 10MiB).
 */
pf_err pf_decode(
    const pf_host_vtable *host,
    void *host_ctx,
    const uint8_t *data,
    size_t len,
    void **out_value,
    uint32_t max_depth,
    size_t max_size
);

/* ---------- fine-grained encode (PHP walks zval and calls these) ---------- */

typedef struct pf_encoder pf_encoder;

pf_encoder *pf_encoder_new(uint8_t flags);
void pf_encoder_free(pf_encoder *enc);

pf_err pf_encoder_null(pf_encoder *enc);
pf_err pf_encoder_bool(pf_encoder *enc, int v);
pf_err pf_encoder_i64(pf_encoder *enc, int64_t v);
pf_err pf_encoder_f64(pf_encoder *enc, double v);
pf_err pf_encoder_string(pf_encoder *enc, const uint8_t *bytes, size_t len);

pf_err pf_encoder_array_packed_begin(pf_encoder *enc, uint32_t count);
pf_err pf_encoder_packed_longs_begin(pf_encoder *enc, uint32_t count);
pf_err pf_encoder_packed_long_item(pf_encoder *enc, int64_t v);
pf_err pf_encoder_array_hash_begin(pf_encoder *enc, uint32_t count);
pf_err pf_encoder_array_rows_begin(pf_encoder *enc, uint32_t nrows, uint32_t ncols);
pf_err pf_encoder_array_repeat_begin(pf_encoder *enc, uint32_t count);
pf_err pf_encoder_key_i64(pf_encoder *enc, int64_t key);
pf_err pf_encoder_key_string(pf_encoder *enc, const uint8_t *bytes, size_t len);

/**
 * Finish encoding. On success *out is malloc'd (pf_free) and *out_len set.
 * Consumes and frees `enc` (do not call pf_encoder_free after success or failure
 * if this returns — always consumes enc).
 */
pf_err pf_encoder_finish(pf_encoder *enc, uint8_t **out, size_t *out_len);

#ifdef __cplusplus
}
#endif

#endif /* PFSERIALIZE_H */
