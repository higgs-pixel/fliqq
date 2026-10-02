// The ONLY file allowed to contain color values (spec 10.1, enforced by test/token_lint_test.dart).
import 'package:flutter/painting.dart';

abstract final class FliqColors {
  static const onyxCanvas = Color(0xFF171721);
  static const graphiteCard = Color(0xFF1E1E2A);
  static const obsidianButton = Color(0xFF272735);
  static const slateBorder = Color(0xFF70707D);
  static const mistBorder = Color(0xFFE2E3ED);
  static const ashText = Color(0xFFC3C3CC);
  static const ivoryText = Color(0xFFEDEDF3);
  static const cobalt = Color(0xFF5266EB);
  static const white = Color(0xFFFFFFFF);
  static const transparent = Color(0x00000000);

  /// Every token, for the contrast and lint tests.
  static const all = <String, Color>{
    'onyxCanvas': onyxCanvas,
    'graphiteCard': graphiteCard,
    'obsidianButton': obsidianButton,
    'slateBorder': slateBorder,
    'mistBorder': mistBorder,
    'ashText': ashText,
    'ivoryText': ivoryText,
    'cobalt': cobalt,
    'white': white,
  };
}

abstract final class FliqRadius {
  static const card = 12.0;
  static const button = 32.0;
  static const pill = 40.0;
}

/// 4 px base; allowed steps (spec 10.4).
abstract final class FliqSpace {
  static const s8 = 8.0;
  static const s12 = 12.0;
  static const s16 = 16.0;
  static const s24 = 24.0;
  static const s32 = 32.0;
  static const s40 = 40.0;
  static const s56 = 56.0;
  static const s72 = 72.0;
}

/// Type scale (spec 10.3).
abstract final class FliqType {
  static const caption = 12.0;
  static const secondary = 14.0;
  static const body = 16.0;
  static const largeBody = 18.0;
  static const subheading = 21.0;
  static const smallHeading = 28.0;
  static const heading = 32.0;
  static const largeHeading = 42.0;
}
