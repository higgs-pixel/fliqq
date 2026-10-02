import 'package:flutter/material.dart';

import 'src/rust/frb_generated.dart';
import 'state/controller.dart';
import 'state/store.dart';
import 'theme/theme.dart';
import 'ui/shell.dart';

Future<void> main() async {
  WidgetsFlutterBinding.ensureInitialized();
  await RustLib.init();
  final store = await Store.open();
  runApp(FliqApp(store: store, controller: TransferController(store)));
}

class FliqApp extends StatelessWidget {
  const FliqApp({super.key, required this.store, required this.controller});
  final Store store;
  final TransferController controller;

  @override
  Widget build(BuildContext context) => MaterialApp(
        title: 'Fliq',
        debugShowCheckedModeBanner: false,
        theme: buildTheme(),
        themeMode: ThemeMode.dark,
        home: DesktopShell(store: store, controller: controller),
      );
}
