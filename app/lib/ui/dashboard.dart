import 'dart:math' as math;

import 'package:file_selector/file_selector.dart';
import 'package:flutter/material.dart';

import '../src/rust/api/fliq.dart';
import '../state/controller.dart';
import '../state/format.dart';
import '../state/store.dart';
import '../strings.dart';
import '../theme/theme.dart';
import '../theme/tokens.dart';
import 'shell.dart';
import 'widgets.dart';

class Dashboard extends StatelessWidget {
  const Dashboard({super.key, required this.store, required this.controller, required this.go});
  final Store store;
  final TransferController controller;
  final ValueChanged<Nav> go;

  @override
  Widget build(BuildContext context) {
    final facts = controller.facts();
    return PageBody(children: [
      Text(S.dashTitle, style: FliqText.largeHeading),
      const SizedBox(height: FliqSpace.s16),
      Text(S.dashSubtitle, style: FliqText.largeBody),
      const SizedBox(height: FliqSpace.s56),
      LayoutBuilder(builder: (context, c) {
        final send = _ActionCard(
          icon: Icons.upload_outlined,
          title: S.sendTitle,
          line: S.sendLine,
          button: PrimaryButton(label: S.sendAction, onPressed: () => go(Nav.send)),
        );
        final receive = _ActionCard(
          icon: Icons.download_outlined,
          title: S.receiveTitle,
          line: S.receiveLine,
          button: SecondaryButton(
            label: S.receiveAction,
            onPressed: controller.busy
                ? null
                : () {
                    controller.startReceive();
                    go(Nav.receive);
                  },
          ),
        );
        if (c.maxWidth < 720) {
          return Column(children: [send, const SizedBox(height: FliqSpace.s24), receive]);
        }
        return IntrinsicHeight(
          child: Row(crossAxisAlignment: CrossAxisAlignment.stretch, children: [
            Expanded(child: send),
            const SizedBox(width: FliqSpace.s24),
            Expanded(child: receive),
          ]),
        );
      }),
      const SizedBox(height: FliqSpace.s24),
      _Metrics(store: store, facts: facts),
      const SizedBox(height: FliqSpace.s24),
      SaveDirCard(store: store),
    ]);
  }
}

class _ActionCard extends StatelessWidget {
  const _ActionCard({required this.icon, required this.title, required this.line, required this.button});
  final IconData icon;
  final String title;
  final String line;
  final Widget button;

  @override
  Widget build(BuildContext context) => FliqCard(
        child: Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
          Container(
            width: 64,
            height: 64,
            decoration: BoxDecoration(color: FliqColors.obsidianButton, borderRadius: BorderRadius.circular(FliqRadius.card)),
            child: Icon(icon, size: 30, color: FliqColors.ivoryText),
          ),
          const SizedBox(height: FliqSpace.s24),
          Text(title, style: FliqText.smallHeading),
          const SizedBox(height: FliqSpace.s8),
          Text(line, style: FliqText.bodyAsh),
          const SizedBox(height: FliqSpace.s32),
          button,
        ]),
      );
}

class _Metrics extends StatelessWidget {
  const _Metrics({required this.store, required this.facts});
  final Store store;
  final EngineFacts facts;

  @override
  Widget build(BuildContext context) {
    final ceiling = store.settings.ceilingMbps;
    final tiles = [
      ceiling == null
          ? const MetricTile(label: S.metricCeiling, value: S.notMeasured)
          : MetricTile(label: S.metricCeiling, value: ceiling.toStringAsFixed(2), unit: 'MB/s'),
      MetricTile(
        label: S.metricPayload,
        value: (facts.maxPayloadBytes / math.pow(1024, 3)).toStringAsFixed(2),
        unit: 'GiB',
      ),
      MetricTile(label: S.metricCrypto, value: facts.cryptoLabel),
      MetricTile(
        label: S.metricRam,
        value: (facts.inflightBudgetBytes ~/ (1024 * 1024)).toString(),
        unit: 'MiB · ${S.fixed}',
      ),
    ];
    return FliqCard(
      child: LayoutBuilder(builder: (context, c) {
        final cols = c.maxWidth < 760 ? 2 : 4;
        return Wrap(
          runSpacing: FliqSpace.s32,
          children: [for (final t in tiles) SizedBox(width: c.maxWidth / cols, child: Padding(padding: const EdgeInsets.only(right: FliqSpace.s24), child: t))],
        );
      }),
    );
  }
}

class SaveDirCard extends StatefulWidget {
  const SaveDirCard({super.key, required this.store});
  final Store store;

  @override
  State<SaveDirCard> createState() => _SaveDirCardState();
}

class _SaveDirCardState extends State<SaveDirCard> {
  FolderInfo? info;

  @override
  void initState() {
    super.initState();
    _load();
  }

  Future<void> _load() async {
    try {
      final i = await folderInfo(path: widget.store.settings.saveDir);
      if (mounted) setState(() => info = i);
    } catch (_) {}
  }

  Future<void> _change() async {
    final dir = await getDirectoryPath(initialDirectory: widget.store.settings.saveDir);
    if (dir == null) return;
    widget.store.settings.saveDir = dir;
    await widget.store.saveSettings();
    await _load();
  }

  @override
  Widget build(BuildContext context) {
    final path = widget.store.settings.saveDir;
    final i = info;
    return FliqCard(
      tone: (i?.isFat ?? false) ? CardTone.warning : CardTone.normal,
      padding: const EdgeInsets.symmetric(horizontal: FliqSpace.s32, vertical: FliqSpace.s24),
      child: Row(children: [
        Container(
          width: 48,
          height: 48,
          decoration: BoxDecoration(color: FliqColors.obsidianButton, borderRadius: BorderRadius.circular(FliqRadius.card)),
          child: const Icon(Icons.folder_outlined, color: FliqColors.ashText),
        ),
        const SizedBox(width: FliqSpace.s24),
        Expanded(
          child: Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
            Text(S.targetDir, style: FliqText.caption),
            const SizedBox(height: 4),
            Tooltip(message: path, child: Text(middleEllipsis(path, 64), style: FliqText.body, maxLines: 1)),
            if (i != null) Text(S.free(fmtBytes(i.freeBytes)), style: FliqText.tabularAsh),
            if (i?.isFat ?? false) ...[
              const SizedBox(height: FliqSpace.s8),
              const StatusLine(icon: Icons.info_outline, text: S.fat32Note),
            ],
          ]),
        ),
        const SizedBox(width: FliqSpace.s16),
        SecondaryButton(label: S.change, onPressed: _change),
      ]),
    );
  }
}
