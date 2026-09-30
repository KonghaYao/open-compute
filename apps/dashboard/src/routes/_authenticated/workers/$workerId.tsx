import { createFileRoute, useNavigate } from "@tanstack/react-router";
import {
  WorkerDetailPage,
  type WorkerDetailTab,
} from "../../../features/workers/worker-detail-page";
import { workerDeploymentsQuery } from "../../../lib/query-options";

export const Route = createFileRoute("/_authenticated/workers/$workerId")({
  validateSearch: (
    search: Record<string, unknown>,
  ): {
    tab?: WorkerDetailTab;
  } =>
    search.tab === "deployments" ||
    search.tab === "observability" ||
    search.tab === "settings"
      ? { tab: search.tab }
      : {},
  loader: ({ context, params }) => {
    const { client, instanceId } = context.auth;
    if (!client || !instanceId) return;
    return context.queryClient.ensureQueryData(
      workerDeploymentsQuery(client, instanceId, params.workerId),
    );
  },
  component: WorkerDetailRoute,
});

function WorkerDetailRoute() {
  const { workerId } = Route.useParams();
  const { tab = "overview" } = Route.useSearch();
  const navigate = useNavigate();
  return (
    <WorkerDetailPage
      workerId={workerId}
      tab={tab}
      setTab={(next) =>
        void navigate({
          to: "/workers/$workerId",
          params: { workerId },
          search: next === "overview" ? {} : { tab: next },
        })
      }
    />
  );
}
