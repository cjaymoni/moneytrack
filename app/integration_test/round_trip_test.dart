// The bridge, exercised end to end on a real device or desktop target.
//
// These assertions are about the crossing, not about the rules — `mt-core` has its own
// suite for what a balance is. What is proved here is that Dart can hand rows to Rust and
// read the answer back, that a refusal arrives as a typed error rather than a crash, and
// that the numbers surviving the trip are the ones the core computed.

import 'package:flutter_test/flutter_test.dart';
import 'package:integration_test/integration_test.dart';
import 'package:moneytrack/main.dart';
import 'package:moneytrack/src/rust/api.dart';
import 'package:moneytrack/src/rust/dto.dart';
import 'package:moneytrack/src/rust/enums.dart';
import 'package:moneytrack/src/rust/error.dart';
import 'package:moneytrack/src/rust/frb_generated.dart';
import 'package:moneytrack/src/rust/scalar.dart';
import 'package:moneytrack/src/wiring_check.dart';

void main() {
  IntegrationTestWidgetsFlutterBinding.ensureInitialized();
  setUpAll(() async => RustLib.init());

  test('the balance walk comes back with the running balance after each row', () async {
    final history = await sampleHistory();

    expect(history.currency, 'GHS');
    expect(history.opening.minorUnits, 120000);
    expect(history.entries.length, 1);
    expect(history.closing.minorUnits, 75000);
    expect(history.firstShortfall, isNull);
  });

  test('the same call carried forward is the forecast', () async {
    final outlook = await sampleOutlook();

    // The pending rent is a row with a later date and nothing else. Projected admits it.
    expect(outlook.entries.length, 2);
    expect(outlook.closing.minorUnits, -75000);
    expect(outlook.firstShortfall?.bookedAt, startsWith('2026-11-28'));
  });

  test('typed text becomes minor units in Rust, never in Dart', () async {
    final amount = await parseAmount(input: '12.50', currency: 'GHS');

    expect(amount.minorUnits, 1250);
    expect(amount.currency, 'GHS');
  });

  test('a refusal crosses as the error the core named, not as a crash', () async {
    // Three decimal places in a two-decimal currency.
    await expectLater(
      parseAmount(input: '12.555', currency: 'GHS'),
      throwsA(
        isA<BridgeError_InvalidAmount>().having(
          (error) => error.reason,
          'reason',
          isNot(contains('12.555')),
        ),
      ),
    );
  });

  test('a row belonging to another account is refused by name', () async {
    final stray = TransactionDto(
      id: '00000000-0000-7000-8000-00000000000a',
      accountId: '00000000-0000-7000-8000-0000000000ff',
      direction: Direction.out,
      amount: const MoneyDto(minorUnits: 1000, currency: 'GHS'),
      bookedAt: '2026-11-05T13:20:00Z',
      label: 'Somebody else\'s',
      status: TxStatus.confirmed,
      source: TxSource.manual,
      sync_: sampleTransactions().first.sync_,
    );

    await expectLater(
      walkBalance(
        account: sampleAccount(),
        transactions: [stray],
        basis: BalanceBasis.confirmed,
      ),
      throwsA(isA<BridgeError_Rule>().having((e) => e.rule, 'rule', 'walk')),
    );
  });

  testWidgets('the holding screen shows figures the core walked', (tester) async {
    await tester.pumpWidget(const MoneytrackApp());
    await tester.pumpAndSettle();

    expect(find.text('Balance now: GHS 750.00'), findsOneWidget);
    expect(find.text('After everything expected: -GHS 750.00'), findsOneWidget);
    expect(find.text('Goes negative on 2026-11-28.'), findsOneWidget);
  });
}
