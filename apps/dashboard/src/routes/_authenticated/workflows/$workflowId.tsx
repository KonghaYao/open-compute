import { createFileRoute, useNavigate } from "@tanstack/react-router";
import {
  WorkflowDetailPage,
  type WorkflowDetailTab,
} from "../../../features/workflows/workflow-detail-page";
import { workflowQuery } from "../../../lib/query-options";

export const Route = createFileRoute("/_authenticated/workflows/$workflowId")({
  validateSearch: (
    search: Record<string, unknown>,
  ): {
    tab?: WorkflowDetailTab;
  } =>
    search.tab === "versions" ||
    search.tab === "instances" ||
    search.tab === "settings"
      ? { tab: search.tab }
      : {},
  loader: ({ context, params }) => {
    const { client, instanceId } = context.auth;
    if (!client || !instanceId) return;
    return context.queryClient.ensureQueryData(
      workflowQuery(client, instanceId, params.workflowId),
    );
  },
  component: WorkflowDetailRoute,
});

function WorkflowDetailRoute() {
  const { workflowId } = Route.useParams();
  const { tab = "overview" } = Route.useSearch();
  const navigate = useNavigate();
  return (
    <WorkflowDetailPage
      workflowId={workflowId}
      tab={tab}
      setTab={(next) =>
        void navigate({
          to: "/workflows/$workflowId",
          params: { workflowId },
          search: next === "overview" ? {} : { tab: next },
        })
      }
    />
  );
}
