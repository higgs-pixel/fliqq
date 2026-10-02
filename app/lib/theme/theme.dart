import 'package:flutter/material.dart';
import 'tokens.dart';

const _tabular = [FontFeature.tabularFigures()];

abstract final class FliqText {
  static const _body = TextStyle(fontFamily: 'Inter', fontWeight: FontWeight.w400, height: 1.5, color: FliqColors.ivoryText);
  static const _display =
      TextStyle(fontFamily: 'InterDisplay', fontWeight: FontWeight.w500, height: 1.15, letterSpacing: -0.4, color: FliqColors.ivoryText);

  static final largeHeading = _display.copyWith(fontSize: FliqType.largeHeading, height: 1.1);
  static final heading = _display.copyWith(fontSize: FliqType.heading);
  static final smallHeading = _display.copyWith(fontSize: FliqType.smallHeading);
  static final subheading = _body.copyWith(fontSize: FliqType.subheading, fontWeight: FontWeight.w500, height: 1.2);
  static final largeBody = _body.copyWith(fontSize: FliqType.largeBody, color: FliqColors.ashText);
  static final body = _body.copyWith(fontSize: FliqType.body);
  static final bodyAsh = body.copyWith(color: FliqColors.ashText);
  static final secondary = _body.copyWith(fontSize: FliqType.secondary, color: FliqColors.ashText);
  static final caption = _body.copyWith(fontSize: FliqType.caption, color: FliqColors.ashText, letterSpacing: 0.6);
  static final metric = _display.copyWith(fontSize: FliqType.smallHeading, fontFeatures: _tabular);
  static final tabular = body.copyWith(fontFeatures: _tabular);
  static final tabularAsh = secondary.copyWith(fontFeatures: _tabular);
  static final button = _body.copyWith(fontSize: FliqType.body, fontWeight: FontWeight.w500, height: 1.2);
  static final code = _display.copyWith(fontSize: FliqType.heading, letterSpacing: 6, fontFeatures: _tabular);
}

ThemeData buildTheme() {
  const scheme = ColorScheme.dark(
    primary: FliqColors.cobalt,
    onPrimary: FliqColors.white,
    secondary: FliqColors.obsidianButton,
    onSecondary: FliqColors.ivoryText,
    surface: FliqColors.graphiteCard,
    onSurface: FliqColors.ivoryText,
    error: FliqColors.mistBorder,
    onError: FliqColors.onyxCanvas,
    outline: FliqColors.slateBorder,
    surfaceTint: FliqColors.transparent,
  );
  return ThemeData(
    useMaterial3: true,
    brightness: Brightness.dark,
    colorScheme: scheme,
    scaffoldBackgroundColor: FliqColors.onyxCanvas,
    fontFamily: 'Inter',
    splashColor: FliqColors.obsidianButton,
    highlightColor: FliqColors.obsidianButton,
    hoverColor: FliqColors.obsidianButton,
    focusColor: FliqColors.obsidianButton,
    splashFactory: NoSplash.splashFactory,
    dividerColor: FliqColors.obsidianButton,
    textTheme: TextTheme(bodyMedium: FliqText.body, bodySmall: FliqText.secondary, labelLarge: FliqText.button),
    iconTheme: const IconThemeData(color: FliqColors.ivoryText, size: 22),
    textSelectionTheme: const TextSelectionThemeData(
      cursorColor: FliqColors.cobalt,
      selectionColor: FliqColors.obsidianButton,
      selectionHandleColor: FliqColors.cobalt,
    ),
    tooltipTheme: TooltipThemeData(
      decoration: BoxDecoration(color: FliqColors.obsidianButton, borderRadius: BorderRadius.circular(8)),
      textStyle: FliqText.secondary.copyWith(color: FliqColors.ivoryText),
    ),
    scrollbarTheme: ScrollbarThemeData(thumbColor: WidgetStateProperty.all(FliqColors.obsidianButton)),
    pageTransitionsTheme: const PageTransitionsTheme(builders: {
      TargetPlatform.windows: FadeUpwardsPageTransitionsBuilder(),
      TargetPlatform.android: FadeUpwardsPageTransitionsBuilder(),
    }),
  );
}
