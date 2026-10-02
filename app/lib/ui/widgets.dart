// Section 10.5 components. Colors come only from tokens.dart.
import 'package:flutter/material.dart';

import '../theme/theme.dart';
import '../theme/tokens.dart';

ButtonStyle _base({required Color bg, required Color fg, BorderSide? side}) => ButtonStyle(
      backgroundColor: WidgetStateProperty.resolveWith((s) => s.contains(WidgetState.disabled) ? FliqColors.obsidianButton : bg),
      foregroundColor: WidgetStateProperty.resolveWith((s) => s.contains(WidgetState.disabled) ? FliqColors.ashText : fg),
      overlayColor: WidgetStateProperty.all(FliqColors.transparent),
      elevation: WidgetStateProperty.all(0),
      shadowColor: WidgetStateProperty.all(FliqColors.transparent),
      minimumSize: WidgetStateProperty.all(const Size(48, 48)),
      padding: WidgetStateProperty.all(const EdgeInsets.symmetric(horizontal: FliqSpace.s24, vertical: FliqSpace.s12)),
      shape: WidgetStateProperty.all(RoundedRectangleBorder(borderRadius: BorderRadius.circular(FliqRadius.button))),
      side: WidgetStateProperty.resolveWith((s) {
        if (s.contains(WidgetState.focused)) return const BorderSide(color: FliqColors.cobalt, width: 2);
        if (s.contains(WidgetState.disabled)) return const BorderSide(color: FliqColors.slateBorder);
        return side;
      }),
      textStyle: WidgetStateProperty.all(FliqText.button),
    );

class PrimaryButton extends StatelessWidget {
  const PrimaryButton({super.key, required this.label, this.onPressed, this.icon});
  final String label;
  final VoidCallback? onPressed;
  final IconData? icon;

  @override
  Widget build(BuildContext context) => Semantics(
        button: true,
        label: label,
        child: TextButton(
          style: _base(bg: FliqColors.cobalt, fg: FliqColors.white),
          onPressed: onPressed,
          child: _ButtonContent(label: label, icon: icon),
        ),
      );
}

class SecondaryButton extends StatelessWidget {
  const SecondaryButton({super.key, required this.label, this.onPressed, this.icon, this.outlined = false});
  final String label;
  final VoidCallback? onPressed;
  final IconData? icon;
  final bool outlined;

  @override
  Widget build(BuildContext context) => Semantics(
        button: true,
        label: label,
        child: TextButton(
          style: _base(
            bg: outlined ? FliqColors.transparent : FliqColors.obsidianButton,
            fg: FliqColors.ivoryText,
            side: outlined ? const BorderSide(color: FliqColors.slateBorder) : null,
          ),
          onPressed: onPressed,
          child: _ButtonContent(label: label, icon: icon),
        ),
      );
}

class LowButton extends StatelessWidget {
  const LowButton({super.key, required this.label, this.onPressed});
  final String label;
  final VoidCallback? onPressed;

  @override
  Widget build(BuildContext context) => TextButton(
        style: _base(bg: FliqColors.transparent, fg: FliqColors.ivoryText),
        onPressed: onPressed,
        child: Text(label),
      );
}

class _ButtonContent extends StatelessWidget {
  const _ButtonContent({required this.label, this.icon});
  final String label;
  final IconData? icon;

  @override
  Widget build(BuildContext context) => Row(mainAxisSize: MainAxisSize.min, children: [
        if (icon != null) ...[Icon(icon, size: 20), const SizedBox(width: FliqSpace.s8)],
        Text(label),
      ]);
}

enum CardTone { normal, warning, error }

class FliqCard extends StatelessWidget {
  const FliqCard({super.key, required this.child, this.padding = const EdgeInsets.all(FliqSpace.s32), this.tone = CardTone.normal});
  final Widget child;
  final EdgeInsets padding;
  final CardTone tone;

  @override
  Widget build(BuildContext context) => Container(
        padding: padding,
        decoration: BoxDecoration(
          color: FliqColors.graphiteCard,
          borderRadius: BorderRadius.circular(FliqRadius.card),
          // States without extra colors (spec 10.8): warning = slate outline, error = mist outline.
          border: switch (tone) {
            CardTone.normal => null,
            CardTone.warning => Border.all(color: FliqColors.slateBorder),
            CardTone.error => Border.all(color: FliqColors.mistBorder),
          },
        ),
        child: child,
      );
}

class MetricTile extends StatelessWidget {
  const MetricTile({super.key, required this.label, required this.value, this.unit});
  final String label;
  final String value;
  final String? unit;

  @override
  Widget build(BuildContext context) => Semantics(
        label: '$label: $value ${unit ?? ''}',
        child: ExcludeSemantics(
          child: Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
            Text(label, style: FliqText.caption),
            const SizedBox(height: FliqSpace.s12),
            Wrap(crossAxisAlignment: WrapCrossAlignment.end, spacing: FliqSpace.s8, children: [
              Text(value, style: FliqText.metric),
              if (unit != null) Padding(padding: const EdgeInsets.only(bottom: 4), child: Text(unit!, style: FliqText.secondary)),
            ]),
          ]),
        ),
      );
}

class ProgressPill extends StatelessWidget {
  const ProgressPill({super.key, required this.value});
  final double value;

  @override
  Widget build(BuildContext context) => Semantics(
        value: '${(value * 100).round()}%',
        child: ClipRRect(
          borderRadius: BorderRadius.circular(FliqRadius.pill),
          child: SizedBox(
            height: 8,
            child: LinearProgressIndicator(
              value: value.clamp(0.0, 1.0).toDouble(),
              backgroundColor: FliqColors.obsidianButton,
              valueColor: const AlwaysStoppedAnimation(FliqColors.cobalt),
            ),
          ),
        ),
      );
}

/// Icon + sentence; used for success, warning and error lines (spec 10.8).
class StatusLine extends StatelessWidget {
  const StatusLine({super.key, required this.icon, required this.text, this.strong = false});
  final IconData icon;
  final String text;
  final bool strong;

  @override
  Widget build(BuildContext context) => Row(crossAxisAlignment: CrossAxisAlignment.start, children: [
        Icon(icon, size: 20, color: FliqColors.ivoryText),
        const SizedBox(width: FliqSpace.s8),
        Expanded(child: Text(text, style: strong ? FliqText.body : FliqText.secondary)),
      ]);
}

/// Owner-supplied logo, or the wordmark until it exists.
class FliqLogo extends StatelessWidget {
  const FliqLogo({super.key, this.size = 32});
  final double size;

  @override
  Widget build(BuildContext context) => Image.asset(
        'assets/brand/fliq_logo.png',
        width: size,
        height: size,
        semanticLabel: 'Fliq',
        errorBuilder: (_, __, ___) => SizedBox(
          width: size,
          height: size,
          child: Center(child: Text('F', style: FliqText.subheading)),
        ),
      );
}

class Segmented<T> extends StatelessWidget {
  const Segmented({super.key, required this.value, required this.options, required this.onChanged});
  final T value;
  final Map<T, String> options;
  final ValueChanged<T> onChanged;

  @override
  Widget build(BuildContext context) => Container(
        padding: const EdgeInsets.all(4),
        decoration: BoxDecoration(color: FliqColors.onyxCanvas, borderRadius: BorderRadius.circular(FliqRadius.pill)),
        child: Row(mainAxisSize: MainAxisSize.min, children: [
          for (final e in options.entries)
            Semantics(
              selected: e.key == value,
              button: true,
              child: TextButton(
                style: _base(
                  bg: e.key == value ? FliqColors.obsidianButton : FliqColors.transparent,
                  fg: e.key == value ? FliqColors.ivoryText : FliqColors.ashText,
                ).copyWith(
                  minimumSize: WidgetStateProperty.all(const Size(88, 44)),
                  shape: WidgetStateProperty.all(RoundedRectangleBorder(borderRadius: BorderRadius.circular(FliqRadius.pill))),
                ),
                onPressed: () => onChanged(e.key),
                child: Text(e.value),
              ),
            ),
        ]),
      );
}

/// Centered, width-limited page body for desktop screens.
class PageBody extends StatelessWidget {
  const PageBody({super.key, required this.children});
  final List<Widget> children;

  @override
  Widget build(BuildContext context) => SingleChildScrollView(
        padding: const EdgeInsets.fromLTRB(FliqSpace.s72, FliqSpace.s56, FliqSpace.s72, FliqSpace.s72),
        child: Center(
          child: ConstrainedBox(
            constraints: const BoxConstraints(maxWidth: 1120),
            child: Column(crossAxisAlignment: CrossAxisAlignment.stretch, children: children),
          ),
        ),
      );
}
