/*
 * PHP 8.x glue: thin wrappers over the pure-C encoder/decoder.
 * No Rust/FFI on the production path. Checksum payloads are rejected.
 */
#include "php.h"
#include "pfserialize_host.h"
#include "encode_c.h"
#include "decode_c.h"

pf_err pf_php_encode_zval(zval *zv, uint8_t flags, uint8_t **out, size_t *out_len)
{
	return pf_c_encode(zv, flags, out, out_len);
}

pf_err pf_php_decode_to_zval(
	const uint8_t *data,
	size_t len,
	uint32_t max_depth,
	size_t max_size,
	zval *return_value
)
{
	return pf_c_decode(data, len, max_depth, max_size, return_value);
}
