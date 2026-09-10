#ifdef HAVE_CONFIG_H
#include "config.h"
#endif

#include "php.h"
#include "ext/standard/info.h"

#include "php_pfserialize.h"
#include "pfserialize_host.h"
#include "pfserialize.h"

ZEND_BEGIN_ARG_WITH_RETURN_TYPE_MASK_EX(arginfo_pfserialize_encode, 0, 1, MAY_BE_STRING|MAY_BE_FALSE)
	ZEND_ARG_TYPE_INFO(0, value, IS_MIXED, 0)
ZEND_END_ARG_INFO()

ZEND_BEGIN_ARG_WITH_RETURN_TYPE_INFO_EX(arginfo_pfserialize_decode, 0, 1, IS_MIXED, 1)
	ZEND_ARG_TYPE_INFO(0, binary, IS_STRING, 0)
	ZEND_ARG_TYPE_INFO_WITH_DEFAULT_VALUE(0, max_depth, IS_LONG, 0, "100")
	ZEND_ARG_TYPE_INFO_WITH_DEFAULT_VALUE(0, max_size, IS_LONG, 0, "10485760")
ZEND_END_ARG_INFO()

ZEND_BEGIN_ARG_WITH_RETURN_TYPE_MASK_EX(arginfo_pfserialize_size, 0, 1, MAY_BE_LONG|MAY_BE_FALSE)
	ZEND_ARG_TYPE_INFO(0, value, IS_MIXED, 0)
ZEND_END_ARG_INFO()

ZEND_BEGIN_ARG_WITH_RETURN_TYPE_MASK_EX(arginfo_pfserialize_stats, 0, 1, MAY_BE_ARRAY|MAY_BE_FALSE)
	ZEND_ARG_TYPE_INFO(0, value, IS_MIXED, 0)
ZEND_END_ARG_INFO()

static void pf_warn_err(pf_err err)
{
	const char *msg = "unknown error";
	switch (err) {
		case PF_E_BAD_MAGIC: msg = "bad magic"; break;
		case PF_E_UNSUPPORTED_VERSION: msg = "unsupported version"; break;
		case PF_E_UNKNOWN_FLAGS: msg = "unknown flags"; break;
		case PF_E_CHECKSUM: msg = "checksum not supported"; break;
		case PF_E_TRUNCATED: msg = "truncated input"; break;
		case PF_E_TRAILING: msg = "trailing bytes"; break;
		case PF_E_UNKNOWN_TAG: msg = "unknown tag"; break;
		case PF_E_BAD_KEY: msg = "bad array key"; break;
		case PF_E_VARINT: msg = "invalid varint"; break;
		case PF_E_BAD_STRREF: msg = "bad string reference"; break;
		case PF_E_MAX_DEPTH: msg = "max depth exceeded"; break;
		case PF_E_MAX_SIZE: msg = "max size exceeded"; break;
		case PF_E_OVERFLOW: msg = "length/count overflow"; break;
		case PF_E_UNSUPPORTED_TYPE: msg = "unsupported type (object/resource not in V1)"; break;
		case PF_E_HOST: msg = "host callback failed"; break;
		case PF_E_NULL_ARG: msg = "null argument"; break;
		case PF_E_INTERNAL: msg = "internal error"; break;
		default: break;
	}
	php_error_docref(NULL, E_WARNING, "pfserialize: %s (%d)", msg, (int)err);
}

/* {{{ proto string|false pfserialize_encode(mixed $value) */
PHP_FUNCTION(pfserialize_encode)
{
	zval *value;
	uint8_t *buf = NULL;
	size_t len = 0;
	pf_err err;

	ZEND_PARSE_PARAMETERS_START(1, 1)
		Z_PARAM_ZVAL(value)
	ZEND_PARSE_PARAMETERS_END();

	err = pf_php_encode_zval(value, PF_FLAGS_DEFAULT, &buf, &len);
	if (err != PF_OK) {
		pf_warn_err(err);
		RETURN_FALSE;
	}
	RETVAL_STRINGL((const char *)buf, len);
	efree(buf);
}
/* }}} */

/* {{{ proto mixed|false pfserialize_decode(string $binary, int $max_depth = 100, int $max_size = 10485760) */
PHP_FUNCTION(pfserialize_decode)
{
	zend_string *binary;
	zend_long max_depth = 100;
	zend_long max_size = 10 * 1024 * 1024;
	pf_err err;

	ZEND_PARSE_PARAMETERS_START(1, 3)
		Z_PARAM_STR(binary)
		Z_PARAM_OPTIONAL
		Z_PARAM_LONG(max_depth)
		Z_PARAM_LONG(max_size)
	ZEND_PARSE_PARAMETERS_END();

	if (max_depth < 0) {
		max_depth = 0;
	}
	if (max_size < 0) {
		max_size = 0;
	}

	err = pf_php_decode_to_zval(
		(const uint8_t *)ZSTR_VAL(binary),
		ZSTR_LEN(binary),
		(uint32_t)max_depth,
		(size_t)max_size,
		return_value
	);
	if (err != PF_OK) {
		pf_warn_err(err);
		RETURN_FALSE;
	}
}
/* }}} */

/* {{{ proto int|false pfserialize_size(mixed $value) */
PHP_FUNCTION(pfserialize_size)
{
	zval *value;
	uint8_t *buf = NULL;
	size_t len = 0;
	pf_err err;

	ZEND_PARSE_PARAMETERS_START(1, 1)
		Z_PARAM_ZVAL(value)
	ZEND_PARSE_PARAMETERS_END();

	err = pf_php_encode_zval(value, PF_FLAGS_DEFAULT, &buf, &len);
	if (err != PF_OK) {
		pf_warn_err(err);
		RETURN_FALSE;
	}
	efree(buf);
	RETURN_LONG((zend_long)len);
}
/* }}} */

/* {{{ proto array|false pfserialize_stats(mixed $value) */
PHP_FUNCTION(pfserialize_stats)
{
	zval *value;
	uint8_t *buf = NULL;
	size_t len = 0;
	pf_err err;
	size_t original = 0;

	ZEND_PARSE_PARAMETERS_START(1, 1)
		Z_PARAM_ZVAL(value)
	ZEND_PARSE_PARAMETERS_END();

	err = pf_php_encode_zval(value, PF_FLAGS_DEFAULT, &buf, &len);
	if (err != PF_OK) {
		pf_warn_err(err);
		RETURN_FALSE;
	}

	/* original_size via native serialize() for comparison */
	{
		zval fname, retval;
		ZVAL_STRING(&fname, "serialize");
		zval params[1];
		ZVAL_COPY(&params[0], value);
		ZVAL_UNDEF(&retval);
		if (call_user_function(NULL, NULL, &fname, &retval, 1, params) == SUCCESS
			&& Z_TYPE(retval) == IS_STRING) {
			original = Z_STRLEN(retval);
		}
		zval_ptr_dtor(&params[0]);
		zval_ptr_dtor(&fname);
		zval_ptr_dtor(&retval);
	}

	array_init(return_value);
	add_assoc_long(return_value, "original_size", (zend_long)original);
	add_assoc_long(return_value, "serialized_size", (zend_long)len);
	if (original > 0) {
		add_assoc_double(return_value, "compression_ratio", (double)len / (double)original);
	} else {
		add_assoc_double(return_value, "compression_ratio", 0.0);
	}
	add_assoc_long(return_value, "string_count", -1); /* filled later if needed */

	efree(buf);
}
/* }}} */

static const zend_function_entry pfserialize_functions[] = {
	PHP_FE(pfserialize_encode, arginfo_pfserialize_encode)
	PHP_FE(pfserialize_decode, arginfo_pfserialize_decode)
	PHP_FE(pfserialize_size, arginfo_pfserialize_size)
	PHP_FE(pfserialize_stats, arginfo_pfserialize_stats)
	PHP_FE_END
};

PHP_MINIT_FUNCTION(pfserialize)
{
	return SUCCESS;
}

PHP_MINFO_FUNCTION(pfserialize)
{
	php_info_print_table_start();
	php_info_print_table_header(2, "pfserialize support", "enabled");
	php_info_print_table_row(2, "Version", PHP_PFSERIALIZE_VERSION);
	php_info_print_table_row(2, "PHP target", "8.x");
	php_info_print_table_row(2, "Encode path", "C inline");
	php_info_print_table_row(2, "Decode path", "C inline");
	php_info_print_table_row(2, "Checksum", "rejected (flag reserved)");
	php_info_print_table_row(2, "Wire format", "PFBinary v1");
	php_info_print_table_end();
}

zend_module_entry pfserialize_module_entry = {
	STANDARD_MODULE_HEADER,
	"pfserialize",
	pfserialize_functions,
	PHP_MINIT(pfserialize),
	NULL,
	NULL,
	NULL,
	PHP_MINFO(pfserialize),
	PHP_PFSERIALIZE_VERSION,
	STANDARD_MODULE_PROPERTIES
};

#ifdef COMPILE_DL_PFSERIALIZE
# ifdef ZTS
ZEND_TSRMLS_CACHE_DEFINE()
# endif
ZEND_GET_MODULE(pfserialize)
#endif
