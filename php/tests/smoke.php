<?php
declare(strict_types=1);

echo "pfserialize smoke (PHP " . PHP_VERSION . ")\n";

function assert_rt(string $name, mixed $value): void
{
    $bin = pfserialize_encode($value);
    if ($bin === false) {
        fwrite(STDERR, "FAIL encode $name\n");
        exit(1);
    }
    $out = pfserialize_decode($bin);
    if ($out !== $value) {
        fwrite(STDERR, "FAIL roundtrip $name\n");
        var_dump($value, $out);
        exit(1);
    }
    echo sprintf("OK %-14s size=%d\n", $name, strlen($bin));
}

$cases = [
    'null' => null,
    'bool' => true,
    'int' => 10001,
    'neg_int' => -64,
    'float' => 99.99,
    'string' => 'Tom',
    'empty_str' => '',
    'empty_arr' => [],
    'packed' => [1, 2, 3, 4],
    'assoc' => [
        'id' => 10001,
        'name' => 'Tom',
        'price' => 99.99,
        'active' => true,
        'extra' => null,
    ],
    'repeated' => [
        ['id' => 1, 'status' => 'active'],
        ['id' => 2, 'status' => 'active'],
        ['id' => 3, 'status' => 'active'],
    ],
    'neg_keys' => [-1 => 'a', -2 => 'b'],
    'nested' => ['a' => [1, 2], 'b' => ['x' => 'y']],
    'binary' => "a\0b\xff",
];

foreach ($cases as $name => $value) {
    assert_rt($name, $value);
}

$stats = pfserialize_stats($cases['assoc']);
echo "stats ratio=" . $stats['compression_ratio'] . "\n";

$packed = pfserialize_encode([1, 2, 3, 4]);
$expect = '5046424e01020a0402040608';
if (bin2hex($packed) !== $expect) {
    fwrite(STDERR, "FAIL packed hex\n got  " . bin2hex($packed) . "\n want $expect\n");
    exit(1);
}
echo "OK packed protocol hex\n";

// object rejected
$obj = new stdClass();
$r = @pfserialize_encode($obj);
if ($r !== false) {
    fwrite(STDERR, "FAIL expected object encode to fail\n");
    exit(1);
}
echo "OK reject object\n";

// malformed
$bad = @pfserialize_decode("XXXX\x01\x02\x00");
if ($bad !== false) {
    fwrite(STDERR, "FAIL expected bad magic to fail\n");
    exit(1);
}
echo "OK reject bad magic\n";

// max_depth
$deep = [[[[[1]]]]];
$bin = pfserialize_encode($deep);
$r = @pfserialize_decode($bin, 2);
if ($r !== false) {
    fwrite(STDERR, "FAIL expected max_depth to fail\n");
    exit(1);
}
echo "OK max_depth\n";

echo "ALL PASS\n";
