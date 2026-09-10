#ifndef PFSERIALIZE_DECODE_C_H
#define PFSERIALIZE_DECODE_C_H

#include "php.h"
#include "pfserialize.h"

/**
 * Pure-C PFBinary v1 decoder (no Rust/FFI).
 * FLAG_CHECKSUM payloads are rejected (PF_E_CHECKSUM); V1 PHP path has no XXH3.
 * On success, *out is a fully owned zval (caller keeps it).
 * On failure, *out is UNDEF and no partial value leaks.
 */
pf_err pf_c_decode(
	const uint8_t *data,
	size_t len,
	uint32_t max_depth,
	size_t max_size,
	zval *out
);

#endif
