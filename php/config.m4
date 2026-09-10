PHP_ARG_ENABLE([pfserialize],
  [whether to enable pfserialize support],
  [AS_HELP_STRING([--enable-pfserialize],
    [Enable PFSerialize (pure-C PFBinary v1 encoder/decoder)])],
  [no])

if test "$PHP_PFSERIALIZE" != "no"; then
  PHP_NEW_EXTENSION(pfserialize, pfserialize.c host_php8.c decode_c.c encode_c.c, $ext_shared,, -DZEND_ENABLE_STATIC_TSRMLS_CACHE=1 -O3)
fi
