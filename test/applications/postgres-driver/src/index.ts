import { Client } from "./pg-runtime.js";

export default {
  async fetch(
    _request: Request,
    env: { POSTGRES_HOST: string; POSTGRES_PORT: string },
  ) {
    const client = new Client({
      host: env.POSTGRES_HOST,
      port: Number(env.POSTGRES_PORT),
      user: "open_compute",
      database: "open_compute",
      ssl: false,
      connectionTimeoutMillis: 5_000,
    });
    await client.connect();
    try {
      await client.query("BEGIN");
      const committed = await client.query<{ value: number }>(
        "SELECT $1::int AS value",
        [41],
      );
      await client.query("COMMIT");
      await client.query("BEGIN");
      const rolledBack = await client.query<{ value: number }>(
        "SELECT $1::int AS value",
        [42],
      );
      await client.query("ROLLBACK");
      return Response.json({
        committed: committed.rows[0]?.value,
        rolledBack: rolledBack.rows[0]?.value,
      });
    } finally {
      await client.end();
    }
  },
};
