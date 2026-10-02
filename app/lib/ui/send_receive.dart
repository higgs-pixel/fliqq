import 'dart:async';
import 'dart:io';
import 'dart:math' as math;

import 'package:file_selector/file_selector.dart';
import 'package:flutter/material.dart';
import 'package:path/path.dart' as p;
import 'package:qr_flutter/qr_flutter.dart';

import '../platform/windows_firewall.dart';
import '../state/controller.dart';
import '../state/format.dart';
import '../state/store.dart';
import '../strings.dart';
import '../theme/theme.dart';
import '../theme/tokens.dart';
import 'widgets.dart';

// ------------------------------------------------------------------ send

class SendPage extends StatefulWidget {
  const SendPage({super.key, required this.store, required this.controller});
  final Store store;
  final TransferController controller;

  @override
  State<SendPage> createState() => _SendPageState();
}

class _SendPageState extends State<SendPage> {
  final List<(String path, int size)> picked = [];
  bool move = false;

  Future<void> _pick() async {
    final files = await openFiles();
    setState(() {
      for (final f in files) {
        if (picked.any((e) => e.$1 == f.path)) continue;
        try {
          picked.add((f.path, File(f.path).lengthSync()));
        } catch (_) {}
      }
    });
  }

  @override
  Widget build(BuildContext context) => ListenableBuilder(
        listenable: widget.controller,
        builder: (context, _) {
          final c = widget.controller;
          if (c.phase != Phase.idle && c.isSender) {
            return SessionView(controller: c, onRetry: () => c.startSend(picked.map((e) => e.$1).toList(), move: move));
          }
          final total = picked.fold<int>(0, (a, e) => a + e.$2);
          final limit = widget.store.settings.maxTotalBytes;
          final over = total > limit;
          return PageBody(children: [
            Text(S.navSend, style: FliqText.heading),
            const SizedBox(height: FliqSpace.s32),
            FliqCard(
              child: Column(crossAxisAlignment: CrossAxisAlignment.stretch, children: [
                Row(children: [
                  Expanded(child: Text(S.chooseFiles, style: FliqText.subheading)),
                  if (picked.isNotEmpty) LowButton(label: S.clear, onPressed: () => setState(picked.clear)),
                  const SizedBox(width: FliqSpace.s8),
                  SecondaryButton(label: S.addFiles, icon: Icons.add, onPressed: _pick),
                ]),
                const SizedBox(height: FliqSpace.s24),
                if (picked.isEmpty)
                  Text(S.noFiles, style: FliqText.bodyAsh)
                else
                  ConstrainedBox(
                    constraints: const BoxConstraints(maxHeight: 320),
                    child: ListView.separated(
                      shrinkWrap: true,
                      itemCount: picked.length,
                      separatorBuilder: (_, __) => const Divider(height: 1, color: FliqColors.obsidianButton),
                      itemBuilder: (_, i) => FileListRow(
                        name: p.basename(picked[i].$1),
                        meta: fmtBytes(picked[i].$2),
                        trailing: IconButton(
                          tooltip: S.clear,
                          icon: const Icon(Icons.close, size: 20, color: FliqColors.ashText),
                          onPressed: () => setState(() => picked.removeAt(i)),
                        ),
                      ),
                    ),
                  ),
                const SizedBox(height: FliqSpace.s24),
                ProgressPill(value: limit == 0 ? 0.0 : math.min(1.0, total / limit)),
                const SizedBox(height: FliqSpace.s8),
                Text(S.ofLimit(fmtBytes(total), fmtBytes(limit)), style: FliqText.tabularAsh),
                if (over) ...[const SizedBox(height: FliqSpace.s8), const StatusLine(icon: Icons.error_outline, text: S.overLimit, strong: true)],
                const SizedBox(height: FliqSpace.s32),
                Wrap(spacing: FliqSpace.s16, runSpacing: FliqSpace.s16, crossAxisAlignment: WrapCrossAlignment.center, children: [
                  Segmented<bool>(value: move, options: const {false: S.copy, true: S.move}, onChanged: (v) => setState(() => move = v)),
                  if (move) Text(S.moveNote, style: FliqText.secondary),
                ]),
                const SizedBox(height: FliqSpace.s32),
                Align(
                  alignment: Alignment.centerLeft,
                  child: PrimaryButton(
                    label: S.showCode,
                    icon: Icons.qr_code_2,
                    onPressed: picked.isEmpty || over || widget.controller.busy
                        ? null
                        : () => widget.controller.startSend(picked.map((e) => e.$1).toList(), move: move),
                  ),
                ),
              ]),
            ),
          ]);
        },
      );
}

// ------------------------------------------------------------------ receive

class ReceivePage extends StatelessWidget {
  const ReceivePage({super.key, required this.store, required this.controller});
  final Store store;
  final TransferController controller;

  @override
  Widget build(BuildContext context) => ListenableBuilder(
        listenable: controller,
        builder: (context, _) {
          if (controller.phase != Phase.idle && !controller.isSender) {
            return SessionView(controller: controller, onRetry: controller.startReceive);
          }
          return PageBody(children: [
            Text(S.navReceive, style: FliqText.heading),
            const SizedBox(height: FliqSpace.s32),
            FliqCard(
              child: Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
                const Icon(Icons.download_outlined, size: 30),
                const SizedBox(height: FliqSpace.s24),
                Text(S.receiveTitle, style: FliqText.smallHeading),
                const SizedBox(height: FliqSpace.s8),
                Text(S.receiveLine, style: FliqText.bodyAsh),
                const SizedBox(height: FliqSpace.s8),
                Text(middleEllipsis(store.settings.saveDir, 70), style: FliqText.secondary),
                const SizedBox(height: FliqSpace.s32),
                PrimaryButton(
                  label: S.receiveAction,
                  icon: Icons.qr_code_2,
                  onPressed: controller.busy ? null : controller.startReceive,
                ),
              ]),
            ),
          ]);
        },
      );
}

// ------------------------------------------------------------------ session

class SessionView extends StatelessWidget {
  const SessionView({super.key, required this.controller, required this.onRetry});
  final TransferController controller;
  final VoidCallback onRetry;

  @override
  Widget build(BuildContext context) {
    final c = controller;
    final title = c.isSender ? S.navSend : S.navReceive;
    final body = switch (c.phase) {
      Phase.pairing => _Pairing(c: c, onRetry: onRetry),
      Phase.connected => _Connected(c: c),
      Phase.offer => _Confirm(c: c),
      Phase.transferring => _Transfer(c: c),
      Phase.done => _Done(c: c),
      Phase.failed => _Failed(c: c, onRetry: onRetry),
      Phase.idle => const SizedBox.shrink(),
    };
    return PageBody(children: [Text(title, style: FliqText.heading), const SizedBox(height: FliqSpace.s32), body]);
  }
}

class _Pairing extends StatelessWidget {
  const _Pairing({required this.c, required this.onRetry});
  final TransferController c;
  final VoidCallback onRetry;

  @override
  Widget build(BuildContext context) {
    final uri = c.uri;
    return Column(crossAxisAlignment: CrossAxisAlignment.stretch, children: [
      FliqCard(
        child: Wrap(spacing: FliqSpace.s40, runSpacing: FliqSpace.s32, children: [
          if (uri != null) QrPanel(data: uri),
          ConstrainedBox(
            constraints: const BoxConstraints(maxWidth: 420),
            child: Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
              Text(S.scanTitle, style: FliqText.smallHeading),
              const SizedBox(height: FliqSpace.s16),
              const StatusLine(icon: Icons.visibility_off_outlined, text: S.qrWarning),
              const SizedBox(height: FliqSpace.s24),
              if (c.expires != null) _Countdown(expires: c.expires!, onRetry: onRetry),
              const SizedBox(height: FliqSpace.s24),
              Text(S.addresses, style: FliqText.caption),
              const SizedBox(height: 4),
              Text(c.addresses.join('  ·  '), style: FliqText.tabularAsh),
              const SizedBox(height: FliqSpace.s32),
              SecondaryButton(label: S.cancel, outlined: true, onPressed: c.reset),
            ]),
          ),
        ]),
      ),
      if (c.firewallIssue || c.reachabilityIssue) ...[
        const SizedBox(height: FliqSpace.s24),
        FliqCard(
          tone: CardTone.warning,
          padding: const EdgeInsets.all(FliqSpace.s24),
          child: Row(children: [
            const Expanded(child: StatusLine(icon: Icons.shield_outlined, text: S.firewallWarn, strong: true)),
            if (c.firewallIssue) SecondaryButton(label: S.firewallFix, onPressed: c.fixFirewall),
          ]),
        ),
      ],
    ]);
  }
}

class _Countdown extends StatefulWidget {
  const _Countdown({required this.expires, required this.onRetry});
  final DateTime expires;
  final VoidCallback onRetry;

  @override
  State<_Countdown> createState() => _CountdownState();
}

class _CountdownState extends State<_Countdown> {
  late final Timer t;

  @override
  void initState() {
    super.initState();
    t = Timer.periodic(const Duration(seconds: 1), (_) => setState(() {}));
  }

  @override
  void dispose() {
    t.cancel();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final left = widget.expires.difference(DateTime.now());
    if (left.isNegative) {
      return Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
        const StatusLine(icon: Icons.timer_off_outlined, text: S.expired, strong: true),
        const SizedBox(height: FliqSpace.s16),
        PrimaryButton(label: S.newCode, onPressed: widget.onRetry),
      ]);
    }
    return Text(S.expiresIn(fmtClock(left)), style: FliqText.tabular);
  }
}

/// White panel, onyx modules, error correction M, 4-module quiet zone (spec 10.5).
/// Hidden while the app is in the background (spec 5.3).
class QrPanel extends StatefulWidget {
  const QrPanel({super.key, required this.data, this.size = 280});
  final String data;
  final double size;

  @override
  State<QrPanel> createState() => _QrPanelState();
}

class _QrPanelState extends State<QrPanel> {
  bool hidden = false;
  late final AppLifecycleListener _l;

  @override
  void initState() {
    super.initState();
    _l = AppLifecycleListener(
      onHide: () => setState(() => hidden = true),
      onShow: () => setState(() => hidden = false),
    );
  }

  @override
  void dispose() {
    _l.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final s = widget.size;
    if (hidden) {
      return Container(
        width: s,
        height: s,
        decoration: BoxDecoration(color: FliqColors.obsidianButton, borderRadius: BorderRadius.circular(FliqRadius.card)),
        padding: const EdgeInsets.all(FliqSpace.s24),
        child: Center(child: Text(S.qrHidden, style: FliqText.secondary, textAlign: TextAlign.center)),
      );
    }
    return Semantics(
      label: 'Pairing QR code',
      child: Container(
        width: s,
        height: s,
        decoration: BoxDecoration(color: FliqColors.white, borderRadius: BorderRadius.circular(FliqRadius.card)),
        child: QrImageView(
          data: widget.data,
          version: QrVersions.auto,
          errorCorrectionLevel: QrErrorCorrectLevel.M,
          backgroundColor: FliqColors.white,
          // padding sized for a >= 4-module quiet zone at typical versions
          padding: EdgeInsets.all(s / 14),
          eyeStyle: const QrEyeStyle(eyeShape: QrEyeShape.square, color: FliqColors.onyxCanvas),
          dataModuleStyle: const QrDataModuleStyle(dataModuleShape: QrDataModuleShape.square, color: FliqColors.onyxCanvas),
        ),
      ),
    );
  }
}

class _CodeBlock extends StatelessWidget {
  const _CodeBlock({required this.code});
  final String code;

  @override
  Widget build(BuildContext context) => Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
        Text(S.verifyTitle.toUpperCase(), style: FliqText.caption),
        const SizedBox(height: FliqSpace.s8),
        Semantics(
          label: '${S.verifyTitle} ${code.split('').join(' ')}',
          child: ExcludeSemantics(child: Text('${code.substring(0, 3)} ${code.substring(3)}', style: FliqText.code)),
        ),
        const SizedBox(height: FliqSpace.s8),
        Text(S.verifyHint, style: FliqText.secondary),
      ]);
}

class _Connected extends StatelessWidget {
  const _Connected({required this.c});
  final TransferController c;

  @override
  Widget build(BuildContext context) => FliqCard(
        child: Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
          if (c.code != null) _CodeBlock(code: c.code!),
          const SizedBox(height: FliqSpace.s32),
          StatusLine(
            icon: Icons.hourglass_empty,
            text: '${c.isSender ? S.waitingAccept : S.waitingFiles}  ${S.fromDevice(c.peerName)}',
          ),
          const SizedBox(height: FliqSpace.s32),
          SecondaryButton(label: S.cancel, outlined: true, onPressed: c.cancel),
        ]),
      );
}

class _Confirm extends StatelessWidget {
  const _Confirm({required this.c});
  final TransferController c;

  @override
  Widget build(BuildContext context) => FliqCard(
        child: Column(crossAxisAlignment: CrossAxisAlignment.stretch, children: [
          Text(S.confirmTitle, style: FliqText.smallHeading),
          const SizedBox(height: FliqSpace.s8),
          Text(S.fromDevice(c.peerName), style: FliqText.bodyAsh),
          const SizedBox(height: FliqSpace.s32),
          if (c.code != null) _CodeBlock(code: c.code!),
          const SizedBox(height: FliqSpace.s32),
          _FileList(c: c, showStatus: false),
          const SizedBox(height: FliqSpace.s16),
          Text(S.totalLine(fmtBytes(c.total), fmtBytes(c.freeSpace)), style: FliqText.tabularAsh),
          if (c.fat32Warning) ...[const SizedBox(height: FliqSpace.s16), const StatusLine(icon: Icons.info_outline, text: S.fat32, strong: true)],
          const SizedBox(height: FliqSpace.s32),
          Row(children: [
            PrimaryButton(label: S.accept, onPressed: c.accept),
            const SizedBox(width: FliqSpace.s12),
            SecondaryButton(label: S.decline, onPressed: c.decline),
          ]),
        ]),
      );
}

String _bottleneckText(String b) => switch (b) {
      'link' => S.limitedBy(S.bnLink),
      'storage' => S.limitedBy(S.bnStorage),
      'cpu' => S.limitedBy(S.bnCpu),
      _ => '',
    };

class _Transfer extends StatelessWidget {
  const _Transfer({required this.c});
  final TransferController c;

  @override
  Widget build(BuildContext context) {
    final frac = c.total == 0 ? 0.0 : c.done / c.total;
    final hint = _bottleneckText(c.bottleneck);
    return FliqCard(
      child: Column(crossAxisAlignment: CrossAxisAlignment.stretch, children: [
        Row(crossAxisAlignment: CrossAxisAlignment.end, children: [
          Expanded(child: Text(c.reconnecting ? S.reconnecting : (c.isSender ? S.sending : S.receiving), style: FliqText.subheading)),
          Text('${(frac * 100).toStringAsFixed(0)}%', style: FliqText.metric),
        ]),
        const SizedBox(height: FliqSpace.s16),
        ProgressPill(value: frac),
        const SizedBox(height: FliqSpace.s16),
        Wrap(spacing: FliqSpace.s32, runSpacing: FliqSpace.s8, children: [
          Text('${fmtBytes(c.done)} / ${fmtBytes(c.total)}', style: FliqText.tabularAsh),
          Text(fmtSpeed(c.mbps), style: FliqText.tabularAsh),
          if (c.etaSecs != null) Text(S.etaLine(fmtDuration(c.etaSecs!)), style: FliqText.tabularAsh),
          if (hint.isNotEmpty) Text(hint, style: FliqText.secondary),
        ]),
        const SizedBox(height: FliqSpace.s32),
        _FileList(c: c, showStatus: true),
        const SizedBox(height: FliqSpace.s32),
        Align(alignment: Alignment.centerLeft, child: SecondaryButton(label: S.cancel, outlined: true, onPressed: c.cancel)),
      ]),
    );
  }
}

class _Done extends StatelessWidget {
  const _Done({required this.c});
  final TransferController c;

  @override
  Widget build(BuildContext context) {
    final folder = c.files.isNotEmpty && c.files.first.path != null ? p.dirname(c.files.first.path!) : null;
    return FliqCard(
      child: Column(crossAxisAlignment: CrossAxisAlignment.stretch, children: [
        Row(children: [
          const Icon(Icons.check_circle_outline, size: 28),
          const SizedBox(width: FliqSpace.s12),
          Text(S.doneTitle, style: FliqText.smallHeading),
        ]),
        const SizedBox(height: FliqSpace.s8),
        Text(S.doneLine(fmtBytes(c.total), fmtSpeed(c.mbps), fmtDuration(c.secs)), style: FliqText.tabularAsh),
        for (final n in c.moveFailures) ...[const SizedBox(height: FliqSpace.s8), StatusLine(icon: Icons.info_outline, text: S.moveFailed(n))],
        const SizedBox(height: FliqSpace.s32),
        _FileList(c: c, showStatus: true),
        const SizedBox(height: FliqSpace.s32),
        Row(children: [
          PrimaryButton(label: S.done, onPressed: c.reset),
          if (folder != null) ...[
            const SizedBox(width: FliqSpace.s12),
            SecondaryButton(label: S.openFolder, icon: Icons.folder_open_outlined, onPressed: () => openFolder(folder)),
          ],
        ]),
      ]),
    );
  }
}

class _Failed extends StatelessWidget {
  const _Failed({required this.c, required this.onRetry});
  final TransferController c;
  final VoidCallback onRetry;

  @override
  Widget build(BuildContext context) => FliqCard(
        tone: CardTone.error,
        child: Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
          Row(children: [
            const Icon(Icons.error_outline, size: 28),
            const SizedBox(width: FliqSpace.s12),
            Text(S.errorTitle, style: FliqText.smallHeading),
          ]),
          const SizedBox(height: FliqSpace.s16),
          Text(c.errorMessage, style: FliqText.body),
          if (c.errorCode.isNotEmpty) ...[const SizedBox(height: FliqSpace.s8), Text(S.errorCode(c.errorCode), style: FliqText.secondary)],
          const SizedBox(height: FliqSpace.s32),
          Row(children: [
            PrimaryButton(label: S.tryAgain, onPressed: onRetry),
            const SizedBox(width: FliqSpace.s12),
            SecondaryButton(label: S.done, onPressed: c.reset),
          ]),
        ]),
      );
}

class _FileList extends StatelessWidget {
  const _FileList({required this.c, required this.showStatus});
  final TransferController c;
  final bool showStatus;

  @override
  Widget build(BuildContext context) => ConstrainedBox(
        constraints: const BoxConstraints(maxHeight: 320),
        child: ListView.separated(
          shrinkWrap: true,
          itemCount: c.files.length,
          separatorBuilder: (_, __) => const Divider(height: 1, color: FliqColors.obsidianButton),
          itemBuilder: (_, i) {
            final f = c.files[i];
            return FileListRow(
              name: f.name,
              meta: fmtBytes(f.size),
              trailing: showStatus
                  ? Row(mainAxisSize: MainAxisSize.min, children: [
                      Icon(f.verified ? Icons.check : Icons.more_horiz, size: 18, color: f.verified ? FliqColors.ivoryText : FliqColors.ashText),
                      const SizedBox(width: 6),
                      Text(f.verified ? S.verified : S.waiting, style: FliqText.secondary),
                    ])
                  : null,
            );
          },
        ),
      );
}

class FileListRow extends StatelessWidget {
  const FileListRow({super.key, required this.name, required this.meta, this.trailing});
  final String name;
  final String meta;
  final Widget? trailing;

  @override
  Widget build(BuildContext context) => SizedBox(
        height: 60,
        child: Row(children: [
          const Icon(Icons.insert_drive_file_outlined, size: 20, color: FliqColors.ashText),
          const SizedBox(width: FliqSpace.s12),
          Expanded(
            child: Column(mainAxisAlignment: MainAxisAlignment.center, crossAxisAlignment: CrossAxisAlignment.start, children: [
              Tooltip(message: name, child: Text(middleEllipsis(name, 48), style: FliqText.body, maxLines: 1)),
              Text(meta, style: FliqText.tabularAsh),
            ]),
          ),
          if (trailing != null) trailing!,
        ]),
      );
}
