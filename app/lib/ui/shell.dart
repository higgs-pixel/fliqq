import 'package:flutter/material.dart';

import '../state/controller.dart';
import '../state/store.dart';
import '../strings.dart';
import '../theme/theme.dart';
import '../theme/tokens.dart';
import 'dashboard.dart';
import 'pages.dart';
import 'send_receive.dart';
import 'widgets.dart';

enum Nav { dashboard, send, receive, saveLocation, logs }

class DesktopShell extends StatefulWidget {
  const DesktopShell({super.key, required this.store, required this.controller});
  final Store store;
  final TransferController controller;

  @override
  State<DesktopShell> createState() => _DesktopShellState();
}

class _DesktopShellState extends State<DesktopShell> {
  Nav nav = Nav.dashboard;

  void go(Nav n) => setState(() => nav = n);

  @override
  Widget build(BuildContext context) {
    final page = switch (nav) {
      Nav.dashboard => Dashboard(store: widget.store, controller: widget.controller, go: go),
      Nav.send => SendPage(store: widget.store, controller: widget.controller),
      Nav.receive => ReceivePage(store: widget.store, controller: widget.controller),
      Nav.saveLocation => SaveLocationPage(store: widget.store),
      Nav.logs => LogsPage(store: widget.store),
    };
    return Scaffold(
      body: Row(crossAxisAlignment: CrossAxisAlignment.stretch, children: [
        _Sidebar(nav: nav, onSelect: go, controller: widget.controller),
        Expanded(
          child: AnimatedSwitcher(
            duration: MediaQuery.of(context).disableAnimations ? Duration.zero : const Duration(milliseconds: 180),
            switchInCurve: Curves.easeOut,
            child: KeyedSubtree(key: ValueKey(nav), child: page),
          ),
        ),
      ]),
    );
  }
}

class _Sidebar extends StatelessWidget {
  const _Sidebar({required this.nav, required this.onSelect, required this.controller});
  final Nav nav;
  final ValueChanged<Nav> onSelect;
  final TransferController controller;

  @override
  Widget build(BuildContext context) {
    const items = [
      (Nav.dashboard, Icons.dashboard_outlined, S.navDashboard),
      (Nav.send, Icons.upload_outlined, S.navSend),
      (Nav.receive, Icons.download_outlined, S.navReceive),
      (Nav.saveLocation, Icons.folder_outlined, S.navSaveLocation),
      (Nav.logs, Icons.list_alt_outlined, S.navLogs),
    ];
    return Container(
      width: 248,
      color: FliqColors.graphiteCard,
      padding: const EdgeInsets.fromLTRB(FliqSpace.s16, FliqSpace.s32, FliqSpace.s16, FliqSpace.s24),
      child: Column(crossAxisAlignment: CrossAxisAlignment.stretch, children: [
        Padding(
          padding: const EdgeInsets.symmetric(horizontal: FliqSpace.s8),
          child: Row(children: [
            const FliqLogo(size: 32),
            const SizedBox(width: FliqSpace.s12),
            Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
              Text(S.appName, style: FliqText.subheading),
              Text(S.tagline, style: FliqText.caption),
            ]),
          ]),
        ),
        const SizedBox(height: FliqSpace.s40),
        for (final (n, icon, label) in items) _NavPill(icon: icon, label: label, selected: n == nav, onTap: () => onSelect(n)),
        const Spacer(),
        ListenableBuilder(
          listenable: controller,
          builder: (_, __) => controller.busy
              ? Padding(
                  padding: const EdgeInsets.all(FliqSpace.s8),
                  child: Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
                    Text(controller.isSender ? S.sending : S.receiving, style: FliqText.secondary),
                    const SizedBox(height: FliqSpace.s8),
                    ProgressPill(value: controller.total == 0 ? 0.0 : controller.done / controller.total),
                  ]),
                )
              : const SizedBox.shrink(),
        ),
      ]),
    );
  }
}

class _NavPill extends StatelessWidget {
  const _NavPill({required this.icon, required this.label, required this.selected, required this.onTap});
  final IconData icon;
  final String label;
  final bool selected;
  final VoidCallback onTap;

  @override
  Widget build(BuildContext context) => Padding(
        padding: const EdgeInsets.only(bottom: 4),
        child: Semantics(
          selected: selected,
          button: true,
          label: label,
          child: Material(
            color: selected ? FliqColors.obsidianButton : FliqColors.transparent,
            shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(FliqRadius.pill)),
            child: InkWell(
              customBorder: RoundedRectangleBorder(borderRadius: BorderRadius.circular(FliqRadius.pill)),
              onTap: onTap,
              child: SizedBox(
                height: 48,
                child: Padding(
                  padding: const EdgeInsets.symmetric(horizontal: FliqSpace.s16),
                  child: Row(children: [
                    Icon(icon, size: 20, color: selected ? FliqColors.ivoryText : FliqColors.ashText),
                    const SizedBox(width: FliqSpace.s12),
                    Expanded(
                      child: Text(label,
                          style: FliqText.secondary.copyWith(
                              color: selected ? FliqColors.ivoryText : FliqColors.ashText, fontWeight: FontWeight.w500)),
                    ),
                    if (selected)
                      Container(width: 6, height: 6, decoration: const BoxDecoration(color: FliqColors.cobalt, shape: BoxShape.circle)),
                  ]),
                ),
              ),
            ),
          ),
        ),
      );
}
