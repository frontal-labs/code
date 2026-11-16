/**
 * HTTP client for the Frontal Code integration API.
 *
 * Talks to a running `frontal-code` server over HTTP using the global
 * `fetch` (Node 18+). The endpoints and shapes mirror the server's
 * `/v1/integrations` routes.
 */

/** A lightweight listing entry for a registered integration. */
export interface IntegrationSummary {
  id: string;
  version: string;
  display_name: string;
  description: string;
  capability_count: number;
  auth_required: boolean;
  event_count: number;
}

/** A single capability exposed by an integration. */
export interface CapabilitySummary {
  name: string;
  description: string;
  permission_mode: "read_only" | "workspace_write" | "danger_full_access";
  idempotent: boolean;
}

/** A single event emitted by an integration. */
export interface EventSummary {
  name: string;
  description: string;
  signature_required: boolean;
}

/** Authentication requirements for an integration, if any. */
export interface AuthSpec {
  type: "oauth" | "api_key" | "none";
  [key: string]: unknown;
}

/** The full manifest describing an integration. */
export interface IntegrationManifest {
  id: string;
  version: string;
  display_name: string;
  description: string;
  transport: string;
  capabilities: CapabilitySummary[];
  auth: AuthSpec | null;
  events: EventSummary[];
}

/** Error raised when an integration API request fails. */
export class IntegrationError extends Error {
  readonly code: "http" | "server" | "parse";
  readonly status?: number;
  readonly body?: string;
  constructor(message: string, code: "http" | "server" | "parse", status?: number, body?: string) {
    super(message);
    this.name = "IntegrationError";
    this.code = code;
    this.status = status;
    this.body = body;
  }
}

/**
 * A typed client for the Frontal Code integration API.
 *
 * The base URL is normalized (trailing slashes stripped) and each method
 * throws {@link IntegrationError} on transport, server, or parse failures.
 */
export class IntegrationClient {
  private readonly baseUrl: string;

  constructor(baseUrl: string) {
    this.baseUrl = baseUrl.replace(/\/+$/, "");
  }

  /** List all registered integrations. `GET /v1/integrations` */
  async list(): Promise<IntegrationSummary[]> {
    return this.getJson<IntegrationSummary[]>("/v1/integrations");
  }

  /** Fetch the manifest for a single integration. `GET /v1/integrations/{id}` */
  async get(id: string): Promise<IntegrationManifest> {
    return this.getJson<IntegrationManifest>(`/v1/integrations/${encodeURIComponent(id)}`);
  }

  /** List the capabilities of an integration. `GET /v1/integrations/{id}/capabilities` */
  async capabilities(integrationId: string): Promise<CapabilitySummary[]> {
    return this.getJson<CapabilitySummary[]>(
      `/v1/integrations/${encodeURIComponent(integrationId)}/capabilities`,
    );
  }

  /** List the events of an integration. `GET /v1/integrations/{id}/events` */
  async events(integrationId: string): Promise<EventSummary[]> {
    return this.getJson<EventSummary[]>(
      `/v1/integrations/${encodeURIComponent(integrationId)}/events`,
    );
  }

  /**
   * Invoke a capability on an integration. `POST /v1/integrations/{id}/invoke`
   *
   * @param integrationId The integration to invoke against.
   * @param capability The capability name to invoke.
   * @param input The capability input payload.
   */
  async invoke(
    integrationId: string,
    capability: string,
    input: Record<string, unknown>,
  ): Promise<unknown> {
    const url = `${this.baseUrl}/v1/integrations/${encodeURIComponent(integrationId)}/invoke`;
    let res: Response;
    try {
      res = await fetch(url, {
        method: "POST",
        headers: { "content-type": "application/json", accept: "application/json" },
        body: JSON.stringify({ capability, input }),
      });
    } catch (err) {
      throw new IntegrationError(
        `integration request to ${url} failed: ${String(err)}`,
        "http",
      );
    }

    const body = await res.text();
    if (!res.ok) {
      throw new IntegrationError(
        `integration invoke failed with status ${res.status}: ${body}`,
        "server",
        res.status,
        body,
      );
    }
    try {
      return JSON.parse(body) as unknown;
    } catch {
      throw new IntegrationError("integration invoke response was not valid JSON", "parse", res.status, body);
    }
  }

  /** Perform a `GET` request and decode the JSON body. */
  private async getJson<T>(path: string): Promise<T> {
    const url = `${this.baseUrl}${path}`;
    let res: Response;
    try {
      res = await fetch(url, { headers: { accept: "application/json" } });
    } catch (err) {
      throw new IntegrationError(`integration request to ${url} failed: ${String(err)}`, "http");
    }

    const body = await res.text();
    if (!res.ok) {
      throw new IntegrationError(
        `integration request failed with status ${res.status}: ${body}`,
        "server",
        res.status,
        body,
      );
    }
    try {
      return JSON.parse(body) as T;
    } catch {
      throw new IntegrationError("integration response was not valid JSON", "parse", res.status, body);
    }
  }
}
