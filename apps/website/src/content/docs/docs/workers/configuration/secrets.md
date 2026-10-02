---
title: "Secrets"
---

Use project-local cf. Keep secret files outside the repository with owner-only permissions. Values must not appear in argv, package scripts, logs, target records, or application configuration. Bulk uses JSON Merge Patch: secret objects set or replace values, `null` deletes them, and omitted values remain unchanged.

```sh
ocd cf --target staging workers secrets bulk --worker app-staging --body @/secure/secrets.json
ocd cf --target staging workers secrets list --worker app-staging
ocd cf --target staging workers secrets delete API_TOKEN --worker app-staging --force
ocd cf --target staging deploy --mode staging --secrets-file /secure/deploy-secrets.json
```

The target deployer token authorizes management requests. Project code and build scripts can read their process environment, so CI should build without deployment credentials and deploy prebuilt output afterward. Use command help/schema for exact parameters; stdin behavior is not assumed.

Secret mutations retain the immutable Version model. The platform encrypts persisted secrets; reads expose names and types only. Rollback restores the selected Version’s bindings.

```json
{
  "secrets": {
    "API_TOKEN": {
      "name": "API_TOKEN",
      "type": "secret_text",
      "text": "<value>"
    },
    "OLD_SECRET": null
  }
}
```

The bulk file uses the API object above. `deploy --secrets-file` instead takes a flat map of names to string values.
