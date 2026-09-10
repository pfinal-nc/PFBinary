#ifndef PFSERIALIZE_ENCODE_C_H
#define PFSERIALIZE_ENCODE_C_H

#include "php.h"
#include "pfserialize.h"

/**
 * Pure-C PFBinary v1 encoder (no Rust/FFI).
 * FLAG_CHECKSUM is rejected (PF_E_CHECKSUM); V1 PHP path has no XXH3.
 * On success *out is emalloc'd (caller efree); *out_len set.
 */
pf_err pf_c_encode(zval *zv, uint8_t flags, uint8_t **out, size_t *out_len);

#endif
