<?php
/**
 * 纯性能对比：pfserialize vs 原生 serialize / unserialize
 *
 *   php -d extension=php/modules/pfserialize.so php/examples/bench_vs_serialize.php
 *   php -d extension=php/modules/pfserialize.so php/examples/bench_vs_serialize.php 5000
 */

declare(strict_types=1);

if (!function_exists('pfserialize_encode')) {
    fwrite(STDERR, "pfserialize 未加载。用法：\n");
    fwrite(STDERR, "  php -d extension=php/modules/pfserialize.so php/examples/bench_vs_serialize.php\n");
    exit(1);
}

$rounds = isset($argv[1]) ? max(100, (int)$argv[1]) : 3000;

$datasets = [
    'scalar' => [
        'id' => 10001,
        'name' => 'Tom',
        'price' => 99.99,
        'active' => true,
        'extra' => null,
    ],
    'packed_1k' => range(1, 1000),
    'packed_10k' => range(1, 10000),
    'rows_100' => array_map(
        static fn(int $i): array => [
            'id' => $i,
            'status' => 'active',
            'name' => 'user_' . $i,
            'score' => $i * 1.5,
        ],
        range(1, 100)
    ),
    'rows_1k' => array_map(
        static fn(int $i): array => [
            'id' => $i,
            'status' => 'active',
            'name' => 'user_' . $i,
            'score' => $i * 1.5,
        ],
        range(1, 1000)
    ),
    'repeated_keys' => array_fill(0, 50, [
        'id' => 1,
        'status' => 'active',
        'role' => 'member',
    ]),
    'nested' => [
        'users' => array_map(
            static fn(int $i): array => [
                'id' => $i,
                'tags' => ['php', 'redis', 'cache'],
                'meta' => ['v' => 1, 'ok' => true],
            ],
            range(1, 200)
        ),
    ],
];

function bench_ms(callable $fn, int $rounds): float
{
    $fn();
    $fn();
    $t0 = hrtime(true);
    for ($i = 0; $i < $rounds; $i++) {
        $fn();
    }
    return (hrtime(true) - $t0) / 1e6 / $rounds;
}

function speedup(float $baseMs, float $pfMs): string
{
    if ($pfMs <= 0.0) {
        return 'inf';
    }
    return sprintf('%.2fx', $baseMs / $pfMs);
}

echo "=== pfserialize vs serialize 纯性能 (PHP " . PHP_VERSION . ") ===\n";
echo "rounds/op = {$rounds}  (warmup=2)\n\n";

printf(
    "%-14s %8s %8s %8s  %10s %10s %8s  %10s %10s %8s\n",
    'dataset',
    'ser_B',
    'pf_B',
    'size%',
    'ser_enc',
    'pf_enc',
    'enc↑',
    'ser_dec',
    'pf_dec',
    'dec↑'
);
echo str_repeat('-', 110) . "\n";

$summary = [];

foreach ($datasets as $label => $data) {
    $ser = serialize($data);
    $pf = pfserialize_encode($data);
    if ($pf === false) {
        fwrite(STDERR, "encode fail: {$label}\n");
        exit(1);
    }
    $back = pfserialize_decode($pf);
    if (serialize($back) !== $ser) {
        fwrite(STDERR, "roundtrip fail: {$label}\n");
        exit(1);
    }

    $serSize = strlen($ser);
    $pfSize = strlen($pf);

    $serEnc = bench_ms(static fn() => serialize($data), $rounds);
    $pfEnc = bench_ms(static fn() => pfserialize_encode($data), $rounds);
    $serDec = bench_ms(static fn() => unserialize($ser), $rounds);
    $pfDec = bench_ms(static fn() => pfserialize_decode($pf), $rounds);

    $sizePct = $serSize > 0 ? ($pfSize / $serSize * 100.0) : 0.0;
    $encUp = speedup($serEnc, $pfEnc);
    $decUp = speedup($serDec, $pfDec);

    printf(
        "%-14s %8d %8d %7.1f%%  %8.3fms %8.3fms %8s  %8.3fms %8.3fms %8s\n",
        $label,
        $serSize,
        $pfSize,
        $sizePct,
        $serEnc,
        $pfEnc,
        $encUp,
        $serDec,
        $pfDec,
        $decUp
    );

    $summary[] = [
        'label' => $label,
        'size_pct' => $sizePct,
        'enc_ok' => $pfEnc <= $serEnc * 1.2,
        'dec_ok' => $pfDec <= $serDec,
        'enc_up' => $serEnc / max($pfEnc, 1e-12),
        'dec_up' => $serDec / max($pfDec, 1e-12),
    ];
}

echo "\n=== 门禁解读（相对 serialize）===\n";
echo "G1 体积 ≤70%   G2 decode ≥ serialize   G4 encode ≤1.2× serialize\n\n";
foreach ($summary as $s) {
    $g1 = $s['size_pct'] <= 70.0 ? 'PASS' : 'FAIL';
    $g2 = $s['dec_ok'] ? 'PASS' : 'FAIL';
    $g4 = $s['enc_ok'] ? 'PASS' : 'FAIL';
    printf(
        "  %-14s  G1=%s (%.1f%%)  G2=%s (%.2fx)  G4=%s (%.2fx)\n",
        $s['label'],
        $g1,
        $s['size_pct'],
        $g2,
        $s['dec_up'],
        $g4,
        $s['enc_up']
    );
}
echo "\n说明：enc↑/dec↑ >1 表示 pf 更快（serialize 耗时 / pf 耗时）。\n";
