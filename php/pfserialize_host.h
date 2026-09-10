#ifndef PFSERIALIZE_HOST_PHP8_H
#define PFSERIALIZE_HOST_PHP8_H

#include "php.h"
#include "pfserialize.h"

pf_err pf_php_encode_zval(zval *zv, uint8_t flags, uint8_t **out, size_t *out_len);
pf_err pf_php_decode_to_zval(const uint8_t *data, size_t len, uint32_t max_depth, size_t max_size, zval *return_value);

#endif
