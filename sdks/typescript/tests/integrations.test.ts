import { describe, expect, it, vi } from "vitest";
import { IntegrationClient, IntegrationError } from "../src/integrations.js";

describe("IntegrationClient", () => {
  it("normalizes a trailing slash on the base URL", async () => {
    const client = new IntegrationClient("http://localhost:8080/");
    let capturedUrl = "";
    vi.stubGlobal(
      "fetch",
      vi.fn(async (url: RequestInfo | URL) => {
        capturedUrl = String(url);
        return new Response(JSON.stringify([]), { status: 200 });
      }),
    );

    await client.list();

    expect(capturedUrl).toBe("http://localhost:8080/v1/integrations");
    vi.unstubAllGlobals();
  });

  it("strips multiple trailing slashes", async () => {
    const client = new IntegrationClient("http://localhost:8080///");
    let capturedUrl = "";
    vi.stubGlobal(
      "fetch",
      vi.fn(async (url: RequestInfo | URL) => {
        capturedUrl = String(url);
        return new Response(JSON.stringify([]), { status: 200 });
      }),
    );

    await client.list();

    expect(capturedUrl).toBe("http://localhost:8080/v1/integrations");
    vi.unstubAllGlobals();
  });
});

describe("IntegrationError", () => {
  it("carries the error code, status, and body", () => {
    const err = new IntegrationError("boom", "server", 500, '{"error":"boom"}');
    expect(err).toBeInstanceOf(Error);
    expect(err.name).toBe("IntegrationError");
    expect(err.message).toBe("boom");
    expect(err.code).toBe("server");
    expect(err.status).toBe(500);
    expect(err.body).toBe('{"error":"boom"}');
  });

  it("leaves status and body undefined for http errors", () => {
    const err = new IntegrationError("refused", "http");
    expect(err.code).toBe("http");
    expect(err.status).toBeUndefined();
    expect(err.body).toBeUndefined();
  });
});
