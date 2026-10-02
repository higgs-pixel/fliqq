// Settings and the local Transfer Log (spec 6.7, 9.10): stored only on this device.
import 'dart:convert';
import 'dart:io';

import 'package:path/path.dart' as p;
import 'package:path_provider/path_provider.dart';

class AppSettings {
  AppSettings({required this.deviceName, required this.saveDir, this.maxTotalBytes = 50 * 1024 * 1024 * 1024, this.ceilingMbps});

  String deviceName;
  String saveDir;
  int maxTotalBytes;

  /// Latest measured engine ceiling on this device (spec 10.9). Null until measured.
  double? ceilingMbps;

  Map<String, dynamic> toJson() =>
      {'deviceName': deviceName, 'saveDir': saveDir, 'maxTotalBytes': maxTotalBytes, 'ceilingMbps': ceilingMbps};

  static AppSettings fromJson(Map<String, dynamic> j, AppSettings defaults) => AppSettings(
        deviceName: (j['deviceName'] as String?) ?? defaults.deviceName,
        saveDir: (j['saveDir'] as String?) ?? defaults.saveDir,
        maxTotalBytes: (j['maxTotalBytes'] as int?) ?? defaults.maxTotalBytes,
        ceilingMbps: (j['ceilingMbps'] as num?)?.toDouble(),
      );
}

enum LogDirection { sent, received }

enum LogStatus { verified, failed }

class LogEntry {
  LogEntry({required this.time, required this.direction, required this.name, required this.size, required this.mbps, required this.status, this.peer = ''});

  final DateTime time;
  final LogDirection direction;
  final String name;
  final int size;
  final double mbps;
  final LogStatus status;
  final String peer;

  Map<String, dynamic> toJson() => {
        't': time.toIso8601String(),
        'd': direction.name,
        'n': name,
        's': size,
        'm': mbps,
        'st': status.name,
        'p': peer,
      };

  static LogEntry fromJson(Map<String, dynamic> j) => LogEntry(
        time: DateTime.parse(j['t'] as String),
        direction: LogDirection.values.byName(j['d'] as String),
        name: j['n'] as String,
        size: j['s'] as int,
        mbps: (j['m'] as num).toDouble(),
        status: LogStatus.values.byName(j['st'] as String),
        peer: (j['p'] as String?) ?? '',
      );
}

class Store {
  Store._(this._dir, this.settings, this.logs);

  final Directory _dir;
  AppSettings settings;
  List<LogEntry> logs;

  static Future<Store> open() async {
    final dir = await getApplicationSupportDirectory();
    await dir.create(recursive: true);
    final downloads = await getDownloadsDirectory();
    final defaults = AppSettings(
      deviceName: Platform.localHostname,
      saveDir: p.join(downloads?.path ?? dir.path, 'Fliq'),
    );
    var settings = defaults;
    final sf = File(p.join(dir.path, 'settings.json'));
    if (await sf.exists()) {
      try {
        settings = AppSettings.fromJson(jsonDecode(await sf.readAsString()) as Map<String, dynamic>, defaults);
      } catch (_) {/* corrupt settings: fall back to defaults */}
    }
    var logs = <LogEntry>[];
    final lf = File(p.join(dir.path, 'transfer_logs.json'));
    if (await lf.exists()) {
      try {
        logs = (jsonDecode(await lf.readAsString()) as List).map((e) => LogEntry.fromJson(e as Map<String, dynamic>)).toList();
      } catch (_) {}
    }
    return Store._(dir, settings, logs);
  }

  Future<void> saveSettings() =>
      File(p.join(_dir.path, 'settings.json')).writeAsString(jsonEncode(settings.toJson()));

  Future<void> addLogs(Iterable<LogEntry> entries) async {
    logs = [...entries, ...logs].take(2000).toList();
    await File(p.join(_dir.path, 'transfer_logs.json')).writeAsString(jsonEncode(logs.map((e) => e.toJson()).toList()));
  }

  Future<void> clearLogs() async {
    logs = [];
    final f = File(p.join(_dir.path, 'transfer_logs.json'));
    if (await f.exists()) await f.delete();
  }
}
