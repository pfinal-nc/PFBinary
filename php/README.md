# PHP 扩展（PFSerialize / PFBinary v1）

整个仓库仍是 **Rust 定义协议 + 实现权威编解码**（`crates/pf-core` / `pf-format`，供单测与 CLI）。

本目录的 **PHP 扩展生产热路径** 为纯 C（`encode_c.c` / `decode_c.c`）：为过 igbinary 级 decode 门禁，扩展 `.so` **不再链接** `libpfserialize`，也不走 per-node Rust FFI。协议须与 Rust 单测保持一致。

## 构建

```bash
bash php/build.sh
```

产物：`php/modules/pfserialize.so`

## 安装（可选）

```bash
bash php/install.sh          # 复制到 php-config --extension-dir，并写 scan dir ini（可能要 sudo）
# 或手工：
#   extension=/path/to/php/modules/pfserialize.so
```

示例 ini：`php/pfserialize.ini`

## 测试

```bash
bash php/tests/run_all.sh
php -d extension=php/modules/pfserialize.so php/examples/bench_vs_serialize.php
php -d extension=php/modules/pfserialize.so php/examples/workload_bench.php
php -d extension=igbinary -d extension=php/modules/pfserialize.so php/examples/compare.php
```

## API

- `pfserialize_encode(mixed $value): string|false`
- `pfserialize_decode(string $binary, int $max_depth = 100, int $max_size = 10485760): mixed|false`
- `pfserialize_size(mixed $value): int|false`
- `pfserialize_stats(mixed $value): array|false`

V1 不支持 object/resource；checksum flag 载荷会被拒绝。
