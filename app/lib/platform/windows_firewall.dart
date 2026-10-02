// Windows Firewall check and one-click fix (spec 8.2).
//
// Why a rule check and not only the self-connect test: connections from this PC to its own
// address are not filtered by the inbound firewall, so the self-test can pass while other
// devices are blocked. We therefore also confirm an inbound rule for this exe exists.
import 'dart:io';

const ruleName = 'Fliq';

class FirewallStatus {
  const FirewallStatus({required this.supported, required this.ruleFound});
  final bool supported;
  final bool ruleFound;
}

Future<FirewallStatus> checkFirewallRule() async {
  if (!Platform.isWindows) return const FirewallStatus(supported: false, ruleFound: true);
  try {
    final r = await Process.run('netsh', ['advfirewall', 'firewall', 'show', 'rule', 'name=$ruleName', 'verbose']);
    final exe = Platform.resolvedExecutable.toLowerCase();
    final found = r.exitCode == 0 && (r.stdout as String).toLowerCase().contains(exe);
    return FirewallStatus(supported: true, ruleFound: found);
  } catch (_) {
    return const FirewallStatus(supported: true, ruleFound: false);
  }
}

/// Adds the inbound rule through an elevated `netsh` (Windows shows a UAC prompt).
/// Returns true if the rule now exists.
Future<bool> fixFirewallRule() async {
  if (!Platform.isWindows) return true;
  final exe = Platform.resolvedExecutable.replaceAll("'", "''");
  final netshArgs = "advfirewall firewall add rule name=\"$ruleName\" dir=in action=allow protocol=TCP "
      "program=\"$exe\" enable=yes profile=private,public";
  final ps = "\$p = Start-Process -FilePath netsh -ArgumentList '$netshArgs' -Verb RunAs "
      "-WindowStyle Hidden -PassThru -Wait; exit \$p.ExitCode";
  try {
    await Process.run('powershell', ['-NoProfile', '-NonInteractive', '-Command', ps]);
  } catch (_) {
    return false;
  }
  return (await checkFirewallRule()).ruleFound;
}

Future<void> openFolder(String path) async {
  if (Platform.isWindows) {
    await Process.run('explorer', [path]);
  }
}
