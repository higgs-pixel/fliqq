// Drives one pairing + transfer and turns core events into UI state.
import 'dart:async';

import 'package:flutter/foundation.dart';

import '../platform/windows_firewall.dart';
import '../src/rust/api/fliq.dart';
import 'store.dart';

enum Phase { idle, pairing, connected, offer, transferring, done, failed }

class FileRow {
  FileRow(this.name, this.size);
  final String name;
  final int size;
  bool verified = false;
  String? path;
}

class TransferController extends ChangeNotifier {
  TransferController(this.store);

  final Store store;
  FliqSession? _session;
  StreamSubscription<FliqEvent>? _sub;

  Phase phase = Phase.idle;
  bool isSender = false;
  bool moveSources = false;

  // Pairing
  String? uri;
  List<String> addresses = const [];
  DateTime? expires;
  bool reachabilityIssue = false;
  bool firewallIssue = false;
  String? code;
  String peerName = '';

  // Offer / transfer
  List<FileRow> files = [];
  int total = 0;
  int freeSpace = 0;
  bool fat32Warning = false;
  int done = 0;
  double mbps = 0;
  double? etaSecs;
  String bottleneck = 'unknown';
  bool reconnecting = false;
  final List<String> moveFailures = [];

  // Result
  double secs = 0;
  String errorCode = '';
  String errorMessage = '';

  bool get busy => phase != Phase.idle && phase != Phase.done && phase != Phase.failed;

  Settings _settings() => Settings(
        deviceName: store.settings.deviceName,
        saveDir: store.settings.saveDir,
        maxTotalBytes: store.settings.maxTotalBytes,
        moveSources: moveSources,
      );

  EngineFacts facts() => engineFacts(settings: _settings());

  void _start(bool sender, Stream<FliqEvent> Function(FliqSession s) open) {
    _sub?.cancel();
    _resetFields();
    isSender = sender;
    phase = Phase.pairing;
    final s = FliqSession();
    _session = s;
    _sub = open(s).listen(_onEvent, onError: (Object e) => _fail(e.toString()));
    notifyListeners();
  }

  /// Flow A: this PC shows the QR and sends.
  void startSend(List<String> paths, {required bool move}) {
    moveSources = move;
    _start(true, (s) => s.host(settings: _settings(), files: paths));
  }

  /// Flow B: this PC shows the QR and receives.
  void startReceive() {
    moveSources = false;
    _start(false, (s) => s.host(settings: _settings(), files: const []));
  }

  void accept() {
    _session?.decide(accept: true);
    phase = Phase.transferring;
    notifyListeners();
  }

  void decline() {
    _session?.decide(accept: false);
  }

  void cancel() => _session?.cancel();

  void reset() {
    _session?.cancel();
    _sub?.cancel();
    _session = null;
    _resetFields();
    notifyListeners();
  }

  void _resetFields() {
    phase = Phase.idle;
    uri = null;
    addresses = const [];
    expires = null;
    reachabilityIssue = false;
    firewallIssue = false;
    code = null;
    peerName = '';
    files = [];
    total = 0;
    freeSpace = 0;
    fat32Warning = false;
    done = 0;
    mbps = 0;
    etaSecs = null;
    bottleneck = 'unknown';
    reconnecting = false;
    moveFailures.clear();
    secs = 0;
    errorCode = '';
    errorMessage = '';
  }

  Future<void> fixFirewall() async {
    final ok = await fixFirewallRule();
    firewallIssue = !ok;
    notifyListeners();
  }

  void _onEvent(FliqEvent e) {
    switch (e.kind) {
      case EventKind.showQr:
        uri = e.uri;
        addresses = e.addresses;
        expires = DateTime.fromMillisecondsSinceEpoch(e.expiresUnix * 1000);
        checkFirewallRule().then((st) {
          firewallIssue = st.supported && !st.ruleFound;
          notifyListeners();
        });
      case EventKind.selfCheck:
        reachabilityIssue = e.unreachable.isNotEmpty && e.addresses.isEmpty;
      case EventKind.connected:
        code = e.code;
        peerName = e.peerName;
        uri = null; // the QR is spent; stop showing the secret
        phase = Phase.connected;
      case EventKind.offerReceived:
        code = e.code;
        peerName = e.peerName;
        files = e.files.map((f) => FileRow(f.name, f.size)).toList();
        total = e.total;
        freeSpace = e.freeSpace;
        fat32Warning = e.fat32Warning;
        phase = Phase.offer;
      case EventKind.sending:
        files = e.files.map((f) => FileRow(f.name, f.size)).toList();
        total = e.total;
        moveSources = e.moveSources;
      case EventKind.progress:
        phase = Phase.transferring;
        done = e.done;
        total = e.total;
        mbps = e.mbps;
        etaSecs = e.etaSecs;
        bottleneck = e.bottleneck;
      case EventKind.reconnecting:
        reconnecting = true;
      case EventKind.reconnected:
        reconnecting = false;
      case EventKind.fileVerified:
        if (e.index < files.length) files[e.index].verified = true;
      case EventKind.moveFailed:
        moveFailures.add(e.name);
      case EventKind.finished:
        phase = Phase.done;
        done = e.done;
        total = e.total;
        secs = e.secs;
        mbps = e.mbps;
        for (var i = 0; i < e.files.length && i < files.length; i++) {
          files[i].verified = true;
          files[i].path = e.files[i].path;
        }
        if (files.isEmpty) files = e.files.map((f) => FileRow(f.name, f.size)..verified = true).toList();
        _log(LogStatus.verified);
      case EventKind.failed:
        _fail(e.message, code: e.errorCode);
        return;
    }
    notifyListeners();
  }

  void _fail(String message, {String code = ''}) {
    // Bridge errors from host/join arrive as "CODE|message".
    final i = message.indexOf('|');
    if (code.isEmpty && i > 0) {
      code = message.substring(0, i);
      message = message.substring(i + 1);
    }
    final wasTransferring = phase == Phase.transferring || phase == Phase.connected || phase == Phase.offer;
    errorCode = code;
    errorMessage = message;
    phase = Phase.failed;
    reconnecting = false;
    if (wasTransferring && files.isNotEmpty && code != 'E_REJECTED') _log(LogStatus.failed);
    notifyListeners();
  }

  void _log(LogStatus status) {
    final dir = isSender ? LogDirection.sent : LogDirection.received;
    final now = DateTime.now();
    store.addLogs(files.map((f) => LogEntry(
          time: now,
          direction: dir,
          name: f.name,
          size: f.size,
          mbps: mbps,
          status: status == LogStatus.verified && f.verified ? LogStatus.verified : LogStatus.failed,
          peer: peerName,
        )));
  }

  @override
  void dispose() {
    _session?.cancel();
    _sub?.cancel();
    super.dispose();
  }
}
