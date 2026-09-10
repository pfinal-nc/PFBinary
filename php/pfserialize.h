/*
 * PFSerialize — PFBinary v1 C ABI (pure-C build).
 *
 * Kept minimal: error codes + wire flags shared by encode_c.c / decode_c.c.
 * (The former crates/pf-ffi ABI header is gone; the PHP extension now talks
 *  to the pure-C encoder/decoder directly.)
 */
#ifndef PFSERIALIZE_H
#define PFSERIALIZE_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/* Error codes (stable, mirror crates/pf-core::error). */
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

/* Wire flags (header byte). CheckSUM support is removed; the bit is still
 * defined so encoders/decoders reject checksummed payloads explicitly. */
#define PF_FLAG_CHECKSUM  0x01u
#define PF_FLAG_STRDEDUP  0x02u
#define PF_FLAGS_DEFAULT  PF_FLAG_STRDEDUP

#ifdef __cplusplus
}
#endif

#endif /* PFSERIALIZE_H */