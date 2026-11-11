import 'package:flutter/material.dart';
import 'package:moneytrack/src/rust/api.dart';
import 'package:moneytrack/src/rust/frb_generated.dart';
import 'package:moneytrack/src/rust/scalar.dart';
import 'package:moneytrack/src/wiring_check.dart';

Future<void> main() async {
  WidgetsFlutterBinding.ensureInitialized();
  await RustLib.init();
  runApp(const MoneytrackApp());
}

/// A holding screen while the app is built.
///
/// It shows the one thing worth knowing at this stage: that the rules are running. Both
/// figures below are walked by `mt-core` through `mt-bridge`, from rows Dart wrote by
/// hand — there is no store yet, and no design here either. See `lib/src/wiring_check.dart`.
class MoneytrackApp extends StatelessWidget {
  const MoneytrackApp({super.key});

  @override
  Widget build(BuildContext context) {
    return MaterialApp(
      title: 'moneytrack',
      theme: ThemeData(useMaterial3: true),
      home: const WiringCheckPage(),
    );
  }
}

/// Shows the sample ledger walked twice: once over what has happened, once carried forward.
class WiringCheckPage extends StatelessWidget {
  const WiringCheckPage({super.key});

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(title: const Text('moneytrack')),
      body: Center(
        child: FutureBuilder<List<WalkDto>>(
          future: Future.wait([sampleHistory(), sampleOutlook()]),
          builder: (context, snapshot) {
            if (snapshot.hasError) {
              return Text('The core refused: ${snapshot.error}');
            }
            final walks = snapshot.data;
            if (walks == null) {
              return const CircularProgressIndicator();
            }
            return Padding(
              padding: const EdgeInsets.all(24),
              child: Column(
                mainAxisAlignment: MainAxisAlignment.center,
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Text('Balance now: ${_money(walks[0].closing)}'),
                  const SizedBox(height: 8),
                  Text('After everything expected: ${_money(walks[1].closing)}'),
                  const SizedBox(height: 8),
                  Text(_shortfall(walks[1])),
                ],
              ),
            );
          },
        ),
      ),
    );
  }
}

/// Minor units as a decimal string. Display only — arithmetic stays in Rust.
String _money(MoneyDto amount) {
  final minor = amount.minorUnits;
  final negative = minor < 0;
  final units = minor.abs();
  final major = units ~/ 100;
  final cents = (units % 100).toString().padLeft(2, '0');
  return '${negative ? '-' : ''}${amount.currency} $major.$cents';
}

/// What the projected walk says about running out of money.
String _shortfall(WalkDto outlook) {
  final shortfall = outlook.firstShortfall;
  if (shortfall == null) {
    return 'Nothing expected takes this account below zero.';
  }
  return 'Goes negative on ${shortfall.bookedAt.split('T').first}.';
}
