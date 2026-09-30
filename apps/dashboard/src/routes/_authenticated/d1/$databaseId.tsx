import { createFileRoute, useNavigate } from "@tanstack/react-router";
import {
  D1DetailPage,
  type D1DetailTab,
} from "../../../features/d1/d1-detail-page";
import { d1DatabaseQuery } from "../../../lib/query-options";

export const Route = createFileRoute("/_authenticated/d1/$databaseId")({
  validateSearch: (search: Record<string, unknown>): { tab?: D1DetailTab } =>
    search.tab === "console" ||
    search.tab === "transfer" ||
    search.tab === "recovery" ||
    search.tab === "migrations" ||
    search.tab === "settings"
      ? { tab: search.tab }
      : {},
  loader: ({ context, params }) => {
    const { client, instanceId } = context.auth;
    if (!client || !instanceId) return;
    return context.queryClient.ensureQueryData(
      d1DatabaseQuery(client, instanceId, params.databaseId),
    );
  },
  component: D1Route,
});

function D1Route() {
  const { databaseId } = Route.useParams();
  const { tab = "overview" } = Route.useSearch();
  const navigate = useNavigate();
  return (
    <D1DetailPage
      databaseId={databaseId}
      tab={tab}
      setTab={(next) =>
        void navigate({
          to: "/d1/$databaseId",
          params: { databaseId },
          search: next === "overview" ? {} : { tab: next },
        })
      }
    />
  );
}
