import { LayerCard } from "@cloudflare/kumo/components/layer-card";
import { createFileRoute, Link } from "@tanstack/react-router";
import { CloudflareProductIcon } from "../../components/cloudflare-product-icons";
import { PageHeader, Section } from "../../components/dashboard-page";
import { supportedProducts } from "../../lib/supported-products";

export const Route = createFileRoute("/_authenticated/")({
  component: AccountHomePage,
});

function AccountHomePage() {
  return (
    <div className="grid gap-8">
      <PageHeader
        title="Account home"
        description="Manage compute, storage and AI resources on this open-compute installation."
      />
      <Section
        title="Build"
        description="Choose a product to create or manage resources."
      >
        <div className="grid gap-3 sm:grid-cols-2 lg:grid-cols-3">
          {supportedProducts.map((product) => {
            return (
              <LayerCard key={product.href} className="overflow-hidden p-0">
                <Link
                  to={product.href}
                  className="hover:bg-kumo-tint flex h-full min-h-28 gap-3 px-4 py-4"
                >
                  <span className="bg-kumo-info-tint text-kumo-brand flex size-9 shrink-0 items-center justify-center rounded-lg">
                    <CloudflareProductIcon product={product.icon} size={20} />
                  </span>
                  <span className="grid content-start gap-1">
                    <span className="font-medium">{product.name}</span>
                    <span className="text-kumo-subtle text-sm">
                      {product.description}
                    </span>
                  </span>
                </Link>
              </LayerCard>
            );
          })}
        </div>
      </Section>
    </div>
  );
}
