// Number formatting for sizes, speeds and times (decimal units, like the CLI).
String fmtBytes(num b) {
  if (b < 1e3) return '${b.toInt()} B';
  if (b < 1e6) return '${(b / 1e3).toStringAsFixed(0)} KB';
  if (b < 1e9) return '${(b / 1e6).toStringAsFixed(1)} MB';
  return '${(b / 1e9).toStringAsFixed(2)} GB';
}

String fmtSpeed(double mbps) => '${mbps.toStringAsFixed(mbps < 10 ? 1 : 0)} MB/s';

String fmtDuration(double secs) {
  final s = secs.round();
  if (s < 60) return '${s}s';
  final m = s ~/ 60;
  if (m < 60) return '${m}m ${(s % 60).toString().padLeft(2, '0')}s';
  return '${m ~/ 60}h ${(m % 60).toString().padLeft(2, '0')}m';
}

String fmtClock(Duration d) {
  final s = d.inSeconds.clamp(0, 5999);
  return '${s ~/ 60}:${(s % 60).toString().padLeft(2, '0')}';
}

/// Shorten in the middle so the end (extension, last folder) stays visible (spec 10.5).
String middleEllipsis(String s, int max) {
  if (s.length <= max || max < 5) return s;
  final keepEnd = (max * 0.55).floor();
  final keepStart = max - keepEnd - 1;
  return '${s.substring(0, keepStart)}…${s.substring(s.length - keepEnd)}';
}
