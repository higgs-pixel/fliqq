// Spec 10.1 / 10.11: every text/background pair used meets WCAG AA.
import 'dart:math' as math;

import 'package:fliq/theme/tokens.dart';
import 'package:flutter/painting.dart';
import 'package:flutter_test/flutter_test.dart';

double _lin(double c) => c <= 0.03928 ? c / 12.92 : math.pow((c + 0.055) / 1.055, 2.4).toDouble();
double _lum(Color c) => 0.2126 * _lin(c.r) + 0.7152 * _lin(c.g) + 0.0722 * _lin(c.b);
double ratio(Color a, Color b) {
  final la = _lum(a), lb = _lum(b);
  return (math.max(la, lb) + 0.05) / (math.min(la, lb) + 0.05);
}

void main() {
  const c = FliqColors.all;
  const text = [
    ('ivoryText', 'onyxCanvas'),
    ('ivoryText', 'graphiteCard'),
    ('ivoryText', 'obsidianButton'),
    ('ashText', 'onyxCanvas'),
    ('ashText', 'graphiteCard'),
    ('ashText', 'obsidianButton'),
    ('white', 'cobalt'),
  ];
  const graphics = [
    ('cobalt', 'onyxCanvas'),
    ('cobalt', 'graphiteCard'),
    ('cobalt', 'obsidianButton'),
    ('slateBorder', 'graphiteCard'),
    ('slateBorder', 'onyxCanvas'),
    ('mistBorder', 'graphiteCard'),
    ('onyxCanvas', 'white'),
  ];
  for (final (fg, bg) in text) {
    test('text $fg on $bg >= 4.5', () => expect(ratio(c[fg]!, c[bg]!), greaterThanOrEqualTo(4.5)));
  }
  for (final (fg, bg) in graphics) {
    test('graphic $fg on $bg >= 3.0', () => expect(ratio(c[fg]!, c[bg]!), greaterThanOrEqualTo(3.0)));
  }
  test('cobalt is never small text (fails 4.5 on dark surfaces)', () {
    expect(ratio(c['cobalt']!, c['graphiteCard']!), lessThan(4.5));
  });
}
