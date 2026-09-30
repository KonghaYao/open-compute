export interface ClientConfig {
  readonly host: string;
  readonly port: number;
  readonly user: string;
  readonly database: string;
  readonly ssl: false;
  readonly connectionTimeoutMillis: number;
}

export interface QueryResult<Row> {
  readonly rows: readonly Row[];
}

export class Client {
  constructor(config: ClientConfig);
  connect(): Promise<void>;
  query<Row extends Record<string, unknown> = Record<string, unknown>>(
    text: string,
    values?: readonly unknown[],
  ): Promise<QueryResult<Row>>;
  end(): Promise<void>;
}
