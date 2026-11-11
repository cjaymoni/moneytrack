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
/// figures below are walked by `mt-core` through `mt_bridge`, from rows Dart wrote by
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
class WiringCheckPage extends StatefulWidget {
  const WiringCheckPage({super.key});

  @override
  State<WiringCheckPage> createState() => _WiringCheckPageState();
}

class _WiringCheckPageState extends State<WiringCheckPage> {
  /// Held rather than built in `build`, which would re-cross the bridge on every rebuild
  /// — a theme change, a window resize — and flash the spinner each time.
  late final Future<_Figures> _figures = _renderLedger();

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(title: const Text('moneytrack')),
      body: Center(
        child: FutureBuilder<_Figures>(
          future: _figures,
          builder: (context, snapshot) {
            if (snapshot.hasError) {
              return Text('The core refused: ${snapshot.error}');
            }
            final figures = snapshot.data;
            if (figures == null) {
              return const CircularProgressIndicator();
            }
            return Padding(
              padding: const EdgeInsets.all(24),
              child: Column(
                mainAxisAlignment: MainAxisAlignment.center,
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Text('Balance now: ${figures.now}'),
                  const SizedBox(height: 8),
                  Text('After everything expected: ${figures.projected}'),
                  const SizedBox(height: 8),
                  Text(figures.shortfall),
                ],
              ),
            );
          },
        ),
      ),
    );
  }
}

/// Everything the screen shows, already rendered.
///
/// Formatting happens here rather than in `build` because it can refuse — an unknown
/// currency code throws — and a refusal belongs in the error branch beside the walk's own,
/// not in a framework error screen. Nothing on this path can produce one today; rows from
/// a store will.
class _Figures {
  const _Figures({
    required this.now,
    required this.projected,
    required this.shortfall,
  });

  final String now;
  final String projected;
  final String shortfall;
}

Future<_Figures> _renderLedger() async {
  final walks = await Future.wait([sampleHistory(), sampleOutlook()]);
  final history = walks[0];
  final outlook = walks[1];
  return _Figures(
    now: _money(history.closing),
    projected: _money(outlook.closing),
    shortfall: _shortfall(outlook),
  );
}

/// An amount with its currency code in front of it.
///
/// Where the decimal point goes is Rust's answer, not Dart's: the cedi has two places and
/// the CFA franc has none, and that registry lives in the core. All this adds is the code
/// and the space.
String _money(MoneyDto amount) =>
    '${amount.currency} ${formatAmount(amount: amount)}';

/// What the projected walk says about running out of money.
String _shortfall(WalkDto outlook) {
  final shortfall = outlook.firstShortfall;
  if (shortfall == null) {
    return 'Nothing expected takes this account below zero.';
  }
  return 'Goes negative on ${shortfall.bookedAt.split('T').first}.';
}
