import { Navigate, Route, Routes } from "react-router";
import { useAccounts, useMe } from "./api/queries";
import { SignIn } from "./screens/SignIn";
import { Shell } from "./screens/Shell";
import { AccountScreen } from "./screens/AccountScreen";
import { NewAccount } from "./screens/NewAccount";
import { CategoriesScreen } from "./screens/CategoriesScreen";
import { BreakdownScreen } from "./screens/BreakdownScreen";
import { OutlookScreen } from "./screens/OutlookScreen";
import { BudgetsScreen } from "./screens/BudgetsScreen";
import { RecurringScreen } from "./screens/RecurringScreen";
import { Failure, Loading } from "./ui/states";

export function App() {
  const me = useMe();

  if (me.isPending) return <Loading full />;
  if (me.isError) return <Failure full error={me.error} retry={() => void me.refetch()} />;

  if (!me.data) {
    return (
      <Routes>
        <Route path="/register" element={<SignIn mode="register" />} />
        <Route path="/signin" element={<SignIn mode="login" />} />
        <Route path="*" element={<Navigate to="/signin" replace />} />
      </Routes>
    );
  }

  return (
    <Routes>
      <Route element={<Shell user={me.data} />}>
        <Route index element={<Home />} />
        <Route path="accounts/new" element={<NewAccount />} />
        <Route path="accounts/:accountId" element={<AccountScreen />} />
        <Route path="categories" element={<CategoriesScreen />} />
        <Route path="breakdown" element={<BreakdownScreen />} />
        <Route path="outlook" element={<OutlookScreen />} />
        <Route path="budgets" element={<BudgetsScreen />} />
        <Route path="repeating" element={<RecurringScreen />} />
      </Route>
      <Route path="*" element={<Navigate to="/" replace />} />
    </Routes>
  );
}

/** Lands on the first account, or on opening one if there are none yet. */
function Home() {
  const accounts = useAccounts();
  if (accounts.isPending) return <Loading />;
  if (accounts.isError) {
    return <Failure error={accounts.error} retry={() => void accounts.refetch()} />;
  }
  const first = accounts.data[0];
  return <Navigate to={first ? `/accounts/${first.id}` : "/accounts/new"} replace />;
}
