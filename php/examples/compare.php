<?php
/**
 * PFSerialize 对比示例（PHP 8.x）
 *
 * 用法：
 *   bash php/build.sh   # 若尚未编译
 *   DYLD_LIBRARY_PATH=target/release \
 *     php -d extension=php/modules/pfserialize.so php/examples/compare.php
 *
 * Linux 可将 DYLD_LIBRARY_PATH 换成 LD_LIBRARY_PATH。
 */

declare(strict_types=1);

if (!extension_loaded('pfserialize') && !function_exists('pfserialize_encode')) {
    fwrite(STDERR, "pfserialize 扩展未加载。请先：\n");
    fwrite(STDERR, "  bash php/build.sh\n");
    fwrite(STDERR, "  DYLD_LIBRARY_PATH=target/release php -d extension=php/modules/pfserialize.so php/examples/compare.php\n");
    exit(1);
}

$hasIgbinary = function_exists('igbinary_serialize');

// ---------- 测试数据集 ----------
$datasets = [
    'scalar' => [
        'id' => 10001,
        'name' => 'Tom',
        'price' => 99.99,
        'active' => true,
        'extra' => null,
    ],
    'packed_1k' => range(1, 1000),
    'rows_100' => array_map(
        static fn(int $i): array => [
            'id' => $i,
            'status' => 'active',
            'name' => 'user_' . $i,
            'score' => $i * 1.5,
        ],
        range(1, 100)
    ),
    'repeated_keys' => array_fill(0, 50, [
        'id' => 1,
        'status' => 'active',
        'role' => 'member',
    ]),
];

function bench(callable $fn, int $rounds = 1000): float
{
    $fn(); // warmup
    $t0 = hrtime(true);
    for ($i = 0; $i < $rounds; $i++) {
        $fn();
    }
    return (hrtime(true) - $t0) / 1e6 / $rounds; // ms / op
}

function row(string $name, int $size, float $encMs, float $decMs, ?float $baseSize = null): void
{
    $ratio = $baseSize !== null && $baseSize > 0
        ? sprintf('%5.1f%%', $size / $baseSize * 100)
        : '     -';
    printf(
        "  %-12s  size=%6d  (%s of serialize)  encode=%7.3f ms  decode=%7.3f ms\n",
        $name,
        $size,
        $ratio,
        $encMs,
        $decMs
    );
}

echo "=== PFSerialize 对比示例 (PHP " . PHP_VERSION . ") ===\n";
echo "igbinary: " . ($hasIgbinary ? 'yes' : 'no') . "\n\n";

foreach ($datasets as $label => $data) {
    echo "--- dataset: {$label} ---\n";

    // serialize
    $ser = serialize($data);
    $serSize = strlen($ser);
    $serEnc = bench(static fn() => serialize($data));
    $serDec = bench(static fn() => unserialize($ser));

    // json
    $json = json_encode($data, JSON_UNESCAPED_UNICODE);
    $jsonSize = strlen($json);
    $jsonEnc = bench(static fn() => json_encode($data, JSON_UNESCAPED_UNICODE));
    $jsonDec = bench(static fn() => json_decode($json, true));

    // pfserialize
    $pf = pfserialize_encode($data);
    if ($pf === false) {
        fwrite(STDERR, "pfserialize_encode failed for {$label}\n");
        exit(1);
    }
    $pfSize = strlen($pf);
    $pfEnc = bench(static fn() => pfserialize_encode($data));
    $pfDec = bench(static fn() => pfserialize_decode($pf));

    // roundtrip check
    $back = pfserialize_decode($pf);
    if ($back !== $data) {
        fwrite(STDERR, "ROUNDTRIP FAIL: {$label}\n");
        exit(1);
    }

    row('serialize', $serSize, $serEnc, $serDec);
    row('json', $jsonSize, $jsonEnc, $jsonDec, $serSize);
    row('pfserialize', $pfSize, $pfEnc, $pfDec, $serSize);

    if ($hasIgbinary) {
        $ig = igbinary_serialize($data);
        $igSize = strlen($ig);
        $igEnc = bench(static fn() => igbinary_serialize($data));
        $igDec = bench(static fn() => igbinary_unserialize($ig));
        row('igbinary', $igSize, $igEnc, $igDec, $serSize);
    }

    $stats = pfserialize_stats($data);
    echo "  pfserialize_stats: ";
    echo "original={$stats['original_size']} serialized={$stats['serialized_size']} ";
    echo "ratio=" . round($stats['compression_ratio'], 3) . "\n";
    echo "  pf hex (head): " . bin2hex(substr($pf, 0, 16)) . (strlen($pf) > 16 ? '...' : '') . "\n";
    echo "\n";
}

echo "=== 最小用法 ===\n";
$user = ['id' => 10001, 'name' => 'Tom', 'tags' => ['php', 'redis']];
$bin = pfserialize_encode($user);
$out = pfserialize_decode($bin);
echo "encode size: " . strlen($bin) . " bytes\n";
echo "decode ok: " . ($out === $user ? 'yes' : 'no') . "\n";
echo "hex: " . bin2hex($bin) . "\n";
