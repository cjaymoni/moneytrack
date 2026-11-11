/// The smallest ledger that proves the Rust core is reachable from Flutter.
///
/// Every number on the screen in [main] comes from `mt-core` by way of `mt-bridge`: Dart
/// builds the rows, Rust sorts them, walks them and decides when the money runs out. None
/// of this is the app — there is no store behind it and the rows are written here by hand.
/// It exists so that a broken bridge fails loudly at the first `flutter run` rather than
/// in the middle of building a screen.
library;

import 'package:moneytrack/src/rust/api.dart';
import 'package:moneytrack/src/rust/dto.dart';
import 'package:moneytrack/src/rust/enums.dart';
import 'package:moneytrack/src/rust/scalar.dart';

/// A fixed device id, so the rows look like they came from one handset.
const String _deviceId = '0d000000-0000-7000-8000-000000000001';

/// A fixed instant the sample rows were written at.
const String _writtenAt = '2026-11-01T09:00:00Z';

/// The sample account's id.
const String sampleAccountId = '00000000-0000-7000-8000-000000000001';

SyncMetaDto _sync() => const SyncMetaDto(
  createdAt: _writtenAt,
  updatedAt: _writtenAt,
  deviceId: _deviceId,
);

/// Minor units cross as `PlatformInt64`, which is an `int` on a device and a `BigInt` on
/// the web. Passing an `int` compiles everywhere this currently builds and will not
/// compile for web; the portable constructor lives behind frb's generated-code library,
/// so the real data layer should build amounts in one place rather than reach for it
/// here. Noted because web is a phase 5 target, not a never.
MoneyDto _cedis(int minorUnits) =>
    MoneyDto(minorUnits: minorUnits, currency: 'GHS');

/// A mobile money wallet holding GHS 1,200.00 before anything happens to it.
AccountDto sampleAccount() => AccountDto(
  id: sampleAccountId,
  name: 'MoMo',
  kind: AccountKind.momo,
  currency: 'GHS',
  openingBalance: _cedis(120000),
  sortOrder: 0,
  sync_: _sync(),
);

/// One confirmed row behind us and one pending row ahead of us.
///
/// The pending row is the forecast. Nothing marks it as such — it is a row with a later
/// date, and the walk reaches it by sorting.
List<TransactionDto> sampleTransactions() => [
  TransactionDto(
    id: '00000000-0000-7000-8000-000000000002',
    accountId: sampleAccountId,
    direction: Direction.out,
    amount: _cedis(45000),
    bookedAt: '2026-11-05T13:20:00Z',
    label: 'Groceries',
    counterparty: 'Melcom',
    status: TxStatus.confirmed,
    source: TxSource.manual,
    sync_: _sync(),
  ),
  TransactionDto(
    id: '00000000-0000-7000-8000-000000000003',
    accountId: sampleAccountId,
    direction: Direction.out,
    amount: _cedis(150000),
    bookedAt: '2026-11-28T06:00:00Z',
    label: 'Rent',
    counterparty: 'Landlord',
    status: TxStatus.pending,
    source: TxSource.recurring,
    sync_: _sync(),
  ),
];

/// What the account has actually done, counting only rows known to have happened.
Future<WalkDto> sampleHistory() => walkBalance(
  account: sampleAccount(),
  transactions: sampleTransactions(),
  basis: BalanceBasis.confirmed,
);

/// The same walk carried forward through the pending rows — the outlook.
Future<WalkDto> sampleOutlook() => walkBalance(
  account: sampleAccount(),
  transactions: sampleTransactions(),
  basis: BalanceBasis.projected,
);
