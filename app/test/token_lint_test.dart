// Spec 10.11: the build fails if a color value appears outside tokens.dart.
import 'dart:io';

import 'package:flutter_test/flutter_test.dart';

void main() {
  test('no color literals outside lib/theme/tokens.dart', () {
    final bad = <String>[];
    final pattern = RegExp(r'Color\(0x|Color\.fromARGB|Color\.fromRGBO|\bColors\.[a-z]|#[0-9a-fA-F]{6}\b');
    for (final f in Directory('lib').listSync(recursive: true).whereType<File>()) {
      final path = f.path.replaceAll('\\', '/');
      if (!path.endsWith('.dart') || path.endsWith('lib/theme/tokens.dart') || path.contains('lib/src/rust/')) continue;
      final lines = f.readAsLinesSync();
      for (var i = 0; i < lines.length; i++) {
        if (pattern.hasMatch(lines[i])) bad.add('$path:${i + 1}: ${lines[i].trim()}');
      }
    }
    expect(bad, isEmpty, reason: bad.join('\n'));
  });
}
