import 'package:fliq/state/format.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  test('sizes', () {
    expect(fmtBytes(999), '999 B');
    expect(fmtBytes(5000000), '5.0 MB');
    expect(fmtBytes(53687091200), '53.69 GB');
  });
  test('middle ellipsis keeps the extension', () {
    final s = middleEllipsis('a_very_long_file_name_from_the_field_survey_2026.mp4', 24);
    expect(s.length, 24);
    expect(s.endsWith('.mp4'), isTrue);
    expect(middleEllipsis('short.txt', 24), 'short.txt');
  });
  test('durations', () {
    expect(fmtDuration(42), '42s');
    expect(fmtDuration(125), '2m 05s');
    expect(fmtClock(const Duration(seconds: 119)), '1:59');
  });
}
