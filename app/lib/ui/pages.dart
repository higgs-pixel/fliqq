import 'package:flutter/material.dart';

import '../state/format.dart';
import '../state/store.dart';
import '../strings.dart';
import '../theme/theme.dart';
import '../theme/tokens.dart';
import 'dashboard.dart';
import 'send_receive.dart';
import 'widgets.dart';

class SaveLocationPage extends StatelessWidget {
  const SaveLocationPage({super.key, required this.store});
  final Store store;

  @override
  Widget build(BuildContext context) => PageBody(children: [
        Text(S.saveTitle, style: FliqText.heading),
        const SizedBox(height: FliqSpace.s32),
        SaveDirCard(store: store),
        const SizedBox(height: FliqSpace.s16),
        Text(S.longPathNote, style: FliqText.secondary),
      ]);
}

class LogsPage extends StatefulWidget {
  const LogsPage({super.key, required this.store});
  final Store store;

  @override
  State<LogsPage> createState() => _LogsPageState();
}

class _LogsPageState extends State<LogsPage> {
  String q = '';

  @override
  Widget build(BuildContext context) {
    final logs = widget.store.logs.where((l) => q.isEmpty || l.name.toLowerCase().contains(q.toLowerCase())).toList();
    return PageBody(children: [
      Row(children: [
        Expanded(child: Text(S.logsTitle, style: FliqText.heading)),
        SecondaryButton(
          label: S.clearLogs,
          outlined: true,
          onPressed: widget.store.logs.isEmpty
              ? null
              : () async {
                  await widget.store.clearLogs();
                  setState(() {});
                },
        ),
      ]),
      const SizedBox(height: FliqSpace.s8),
      Text(S.logsLocalNote, style: FliqText.secondary),
      const SizedBox(height: FliqSpace.s24),
      SizedBox(
        width: 360,
        child: TextField(
          onChanged: (v) => setState(() => q = v),
          style: FliqText.body,
          decoration: InputDecoration(
            hintText: S.search,
            hintStyle: FliqText.bodyAsh,
            prefixIcon: const Icon(Icons.search, color: FliqColors.ashText, size: 20),
            filled: true,
            fillColor: FliqColors.obsidianButton,
            contentPadding: const EdgeInsets.symmetric(horizontal: FliqSpace.s24, vertical: FliqSpace.s16),
            enabledBorder: OutlineInputBorder(
              borderRadius: BorderRadius.circular(FliqRadius.button),
              borderSide: const BorderSide(color: FliqColors.slateBorder),
            ),
            focusedBorder: OutlineInputBorder(
              borderRadius: BorderRadius.circular(FliqRadius.button),
              borderSide: const BorderSide(color: FliqColors.cobalt, width: 2),
            ),
          ),
        ),
      ),
      const SizedBox(height: FliqSpace.s24),
      FliqCard(
        padding: const EdgeInsets.symmetric(horizontal: FliqSpace.s32, vertical: FliqSpace.s16),
        child: logs.isEmpty
            ? Padding(padding: const EdgeInsets.symmetric(vertical: FliqSpace.s16), child: Text(S.noLogs, style: FliqText.bodyAsh))
            : Column(children: [
                for (var i = 0; i < logs.length; i++) ...[
                  if (i > 0) const Divider(height: 1, color: FliqColors.obsidianButton),
                  FileListRow(
                    name: logs[i].name,
                    meta: '${logs[i].direction == LogDirection.sent ? S.sent : S.received} · ${fmtBytes(logs[i].size)} · '
                        '${fmtSpeed(logs[i].mbps)} · ${_date(logs[i].time)}',
                    trailing: Row(mainAxisSize: MainAxisSize.min, children: [
                      Icon(logs[i].status == LogStatus.verified ? Icons.check : Icons.error_outline, size: 18),
                      const SizedBox(width: 6),
                      Text(logs[i].status == LogStatus.verified ? S.verified : S.failed, style: FliqText.secondary),
                    ]),
                  ),
                ],
              ]),
      ),
    ]);
  }

  static String _date(DateTime t) =>
      '${t.year}-${t.month.toString().padLeft(2, '0')}-${t.day.toString().padLeft(2, '0')} '
      '${t.hour.toString().padLeft(2, '0')}:${t.minute.toString().padLeft(2, '0')}';
}
