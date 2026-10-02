// Every user-visible string lives here (spec 10.11) so it can be translated.
abstract final class S {
  static const appName = 'Fliq';
  static const tagline = 'Offline Transfer';

  // Navigation
  static const navDashboard = 'Dashboard';
  static const navSend = 'Send File';
  static const navReceive = 'Receive File';
  static const navSaveLocation = 'Save Location';
  static const navLogs = 'Transfer Logs';

  // Dashboard
  static const dashTitle = 'High-Speed Offline Transfer';
  static const dashSubtitle = 'Encrypted, device to device. No internet needed.';
  static const sendTitle = 'Send Files';
  static const sendLine = 'Copy or move up to 50 GB.';
  static const sendAction = 'Choose files';
  static const receiveTitle = 'Receive Files';
  static const receiveLine = 'Show a code the other device scans.';
  static const receiveAction = 'Show receive code';
  static const metricCeiling = 'MEASURED ENGINE CEILING';
  static const metricPayload = 'MAX SINGLE PAYLOAD';
  static const metricCrypto = 'CRYPTO INTEGRITY';
  static const metricRam = 'IN-FLIGHT RAM BOUND';
  static const notMeasured = 'Not measured yet';
  static const fixed = 'Fixed';
  static const targetDir = 'TARGET SAVE DIRECTORY';
  static const change = 'Change';
  static String free(String size) => '$size free';

  // Send flow
  static const chooseFiles = 'Choose files';
  static const addFiles = 'Add files';
  static const clear = 'Clear';
  static const copy = 'Copy';
  static const move = 'Move';
  static const moveNote = 'The original will be removed after the file is verified.';
  static String ofLimit(String used, String limit) => '$used of $limit';
  static const overLimit = 'This is more than the transfer limit.';
  static const showCode = 'Show code';
  static const noFiles = 'No files chosen yet.';

  // QR / pairing
  static const scanTitle = 'Scan with Fliq';
  static const qrWarning = 'Only show this to the person you are sending to.';
  static const qrHidden = 'Code hidden while Fliq is in the background.';
  static String expiresIn(String mmss) => 'Expires in $mmss';
  static const expired = 'This code expired. Ask the other device to show a new one.';
  static const newCode = 'Show a new code';
  static const addresses = 'Listening on';
  static const verifyTitle = 'Verification code';
  static const verifyHint = 'Make sure this code matches the other device.';
  static const waitingAccept = 'Waiting for the other device to accept.';
  static const waitingFiles = 'Connected. Waiting for the file list.';
  static const firewallWarn = 'Windows Firewall may be blocking Fliq.';
  static const firewallFix = 'Fix';
  static const firewallFixing = 'Asking Windows for permission…';
  static const firewallFixed = 'Firewall rule added.';

  // Confirm (receiver)
  static const confirmTitle = 'Incoming files';
  static String fromDevice(String name) => 'From $name';
  static String totalLine(String total, String free) => '$total total · $free free here';
  static const fat32 = 'This drive uses FAT32 and cannot store files of 4 GB or more.';
  static const accept = 'Accept';
  static const decline = 'Decline';

  // Transfer
  static const sending = 'Sending…';
  static const receiving = 'Receiving…';
  static const reconnecting = 'Connection lost. Reconnecting…';
  static String etaLine(String eta) => '$eta left';
  static String limitedBy(String what) => 'Limited by $what';
  static const bnLink = 'the Wi-Fi link';
  static const bnStorage = 'storage speed';
  static const bnCpu = 'the processor';
  static const cancel = 'Cancel';
  static const verified = 'Verified';
  static const waiting = 'Waiting';
  static String moveFailed(String name) => 'Couldn\'t move $name to the Recycle Bin. It was copied.';

  // Done / error
  static const doneTitle = 'Transfer complete';
  static String doneLine(String size, String speed, String time) => '$size at $speed in $time';
  static const openFolder = 'Open folder';
  static const done = 'Done';
  static const errorTitle = 'Transfer failed';
  static const tryAgain = 'Try again';
  static String errorCode(String code) => 'Code $code';

  // Save location
  static const saveTitle = 'Save Location';
  static const longPathNote = 'Long file paths are supported.';
  static const fat32Note = 'This drive uses FAT32. Files of 4 GB or more cannot be saved here.';

  // Logs
  static const logsTitle = 'Transfer Logs';
  static const search = 'Search';
  static const clearLogs = 'Clear logs';
  static const noLogs = 'No transfers yet.';
  static const sent = 'Sent';
  static const received = 'Received';
  static const failed = 'Failed';
  static const logsLocalNote = 'Stored only on this PC.';
}
