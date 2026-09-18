import axios, {
  AxiosHeaders,
  type AxiosInstance,
  type AxiosResponse,
  type InternalAxiosRequestConfig,
} from "axios";
import { config } from "./config";
import { logApiCall, logger } from "./log";
import type {
  FrontalCodeCliRequest,
  FrontalCodeCliResponse,
  FrontalCodeCreateTaskRequest,
  FrontalCodeCreateTaskResponse,
  FrontalCodeEventStreamQuery,
  FrontalCodeListTasksQuery,
  FrontalCodeOrphanPolicyQuery,
  FrontalCodeOrphanPolicyResponse,
  FrontalCodePromptRequest,
  FrontalCodeResolveApprovalRequest,
  FrontalCodeSandboxResponse,
  FrontalCodeStatusResponse,
  FrontalCodeTask,
  FrontalCodeUpdateTaskContextRequest,
  SlackBlock,
  SlackBody,
} from "./types";

export class FrontalCodeApiClient {
  private readonly client: AxiosInstance;
  private readonly baseUrl: string;
  private readonly timeout: number;

  constructor() {
    this.baseUrl = config.frontalCode.apiUrl;
    this.timeout = config.frontalCode.timeout;
    const defaultHeaders: Record<string, string> = {
      "Content-Type": "application/json",
      "User-Agent": "frontal-code-slack-bot/1.0.0",
    };
    if (config.frontalCode.apiKey) {
      defaultHeaders["x-api-key"] = config.frontalCode.apiKey;
    }

    this.client = axios.create({
      baseURL: this.baseUrl,
      timeout: this.timeout,
      headers: defaultHeaders,
    });

    // Request interceptor for logging
    this.client.interceptors.request.use(
      (config: InternalAxiosRequestConfig) => {
        logger.debug("Frontal Code API request", {
          method: config.method,
          url: config.url,
        });
        return config;
      },
      (error) => {
        logger.error("Frontal Code API request error", error as Error);
        return Promise.reject(error);
      }
    );

    // Response interceptor for logging
    this.client.interceptors.response.use(
      (response) => {
        const startTime = response.config.headers?.["X-Start-Time"]
          ? Number.parseInt(response.config.headers["X-Start-Time"] as string)
          : 0;
        const duration = startTime ? Date.now() - startTime : 0;
        logApiCall(
          response.config.url || "",
          response.config.method?.toUpperCase() || "GET",
          duration,
          true
        );
        return response;
      },
      (error) => {
        const startTime = error.config?.headers?.["X-Start-Time"]
          ? Number.parseInt(error.config.headers["X-Start-Time"] as string)
          : 0;
        const duration = startTime ? Date.now() - startTime : 0;
        logApiCall(
          error.config?.url || "",
          error.config?.method?.toUpperCase() || "GET",
          duration,
          false
        );
        return Promise.reject(error);
      }
    );
  }

  async submitPrompt(
    request: FrontalCodePromptRequest
  ): Promise<FrontalCodeCliResponse> {
    const startTime = Date.now();

    try {
      const response: AxiosResponse<FrontalCodeCliResponse> =
        await this.client.post("/v1/prompt", request, {
          headers: new AxiosHeaders({
            "X-Start-Time": startTime.toString(),
          }),
        } as InternalAxiosRequestConfig);

      return response.data;
    } catch (error) {
      logger.error(
        "Failed to submit prompt to Frontal Code API",
        error as Error
      );
      throw new Error(`Frontal Code API error: ${(error as Error).message}`);
    }
  }

  async runCliCommand(
    request: FrontalCodeCliRequest
  ): Promise<FrontalCodeCliResponse> {
    const startTime = Date.now();

    try {
      const response: AxiosResponse<FrontalCodeCliResponse> =
        await this.client.post("/v1/cli/run", request, {
          headers: new AxiosHeaders({
            "X-Start-Time": startTime.toString(),
          }),
        } as InternalAxiosRequestConfig);

      return response.data;
    } catch (error) {
      logger.error(
        "Failed to run CLI command via Frontal Code API",
        error as Error
      );
      throw new Error(`Frontal Code API error: ${(error as Error).message}`);
    }
  }

  async getStatus(): Promise<FrontalCodeStatusResponse> {
    const startTime = Date.now();

    try {
      const response: AxiosResponse<FrontalCodeStatusResponse> =
        await this.client.get("/v1/status", {
          headers: new AxiosHeaders({
            "X-Start-Time": startTime.toString(),
          }),
        } as InternalAxiosRequestConfig);

      return response.data;
    } catch (error) {
      logger.error(
        "Failed to get status from Frontal Code API",
        error as Error
      );
      throw new Error(`Frontal Code API error: ${(error as Error).message}`);
    }
  }

  async getSandboxStatus(): Promise<FrontalCodeSandboxResponse> {
    const startTime = Date.now();

    try {
      const response: AxiosResponse<FrontalCodeSandboxResponse> =
        await this.client.get("/v1/sandbox", {
          headers: new AxiosHeaders({
            "X-Start-Time": startTime.toString(),
          }),
        } as InternalAxiosRequestConfig);

      return response.data;
    } catch (error) {
      logger.error(
        "Failed to get sandbox status from Frontal Code API",
        error as Error
      );
      throw new Error(`Frontal Code API error: ${(error as Error).message}`);
    }
  }

  async getVersion(): Promise<{
    version: string;
    commit: string;
    build_time: string;
  }> {
    const startTime = Date.now();

    try {
      const response = await this.client.get("/v1/version", {
        headers: new AxiosHeaders({
          "X-Start-Time": startTime.toString(),
        }),
      } as InternalAxiosRequestConfig);

      return response.data;
    } catch (error) {
      logger.error(
        "Failed to get version from Frontal Code API",
        error as Error
      );
      throw new Error(`Frontal Code API error: ${(error as Error).message}`);
    }
  }

  async healthCheck(): Promise<boolean> {
    try {
      const response = await this.client.get("/health");
      return response.status === 200;
    } catch (error) {
      logger.error("Frontal Code API health check failed", error as Error);
      return false;
    }
  }

  async createTask(
    request: FrontalCodeCreateTaskRequest
  ): Promise<FrontalCodeCreateTaskResponse> {
    try {
      const response: AxiosResponse<FrontalCodeCreateTaskResponse> =
        await this.client.post("/v1/tasks", request);
      return response.data;
    } catch (error) {
      logger.error("Task creation failed", error as Error);
      throw error;
    }
  }

  async getTask(taskId: string): Promise<FrontalCodeTask> {
    try {
      const response: AxiosResponse<FrontalCodeTask> = await this.client.get(
        `/v1/tasks/${taskId}`
      );
      return response.data;
    } catch (error) {
      logger.error("Task lookup failed", error as Error, { taskId });
      throw error;
    }
  }

  async listTasks(
    query: FrontalCodeListTasksQuery = {}
  ): Promise<FrontalCodeTask[]> {
    try {
      const response: AxiosResponse<FrontalCodeTask[]> = await this.client.get(
        "/v1/tasks",
        {
          params: query,
        }
      );
      return response.data;
    } catch (error) {
      logger.error("Task listing failed", error as Error, { ...query });
      throw error;
    }
  }

  async getOrphanPolicy(
    query: FrontalCodeOrphanPolicyQuery = {}
  ): Promise<FrontalCodeOrphanPolicyResponse> {
    try {
      const response: AxiosResponse<FrontalCodeOrphanPolicyResponse> =
        await this.client.get("/v1/policies/orphans", {
          params: query,
        });
      return response.data;
    } catch (error) {
      logger.error("Orphan policy lookup failed", error as Error, { ...query });
      throw error;
    }
  }

  async updateTaskContext(
    request: FrontalCodeUpdateTaskContextRequest
  ): Promise<FrontalCodeTask> {
    try {
      const response: AxiosResponse<FrontalCodeTask> = await this.client.post(
        `/v1/tasks/${request.taskId}/context`,
        {
          source: request.source,
          user_id: request.user_id,
          channel_id: request.channel_id,
          thread_ts: request.thread_ts,
          approval_message_ts: request.approval_message_ts,
        }
      );
      return response.data;
    } catch (error) {
      logger.error("Task context update failed", error as Error, {
        taskId: request.taskId,
      });
      throw error;
    }
  }

  getEventsWebSocketUrl(query: FrontalCodeEventStreamQuery = {}): string {
    const url = new URL(this.baseUrl);
    url.protocol = url.protocol === "https:" ? "wss:" : "ws:";
    url.pathname = "/v1/events/ws";
    url.search = "";
    for (const [key, value] of Object.entries(query)) {
      if (value === undefined || value === null || value === "") {
        continue;
      }
      url.searchParams.set(key, String(value));
    }
    url.hash = "";
    return url.toString();
  }

  getEventsWebSocketHeaders(): Record<string, string> | undefined {
    if (!config.frontalCode.apiKey) {
      return undefined;
    }
    return {
      "x-api-key": config.frontalCode.apiKey,
    };
  }

  async sendConnectorInteraction(
    connector: string,
    request: {
      action: string;
      value?: string;
      userId: string;
      context: SlackBody;
    }
  ): Promise<{ blocks: SlackBlock[] }> {
    try {
      const response = await this.client.post(
        `/v1/connectors/${encodeURIComponent(connector)}/interactions`,
        {
          action: request.action,
          value: request.value,
          user_id: request.userId,
          context: request.context,
        }
      );
      return response.data;
    } catch (error) {
      logger.error("Slack interaction handling failed", error as Error);
      throw error;
    }
  }

  async resolveTaskApproval(
    request: FrontalCodeResolveApprovalRequest
  ): Promise<FrontalCodeTask> {
    try {
      const response: AxiosResponse<FrontalCodeTask> = await this.client.post(
        `/v1/tasks/${request.taskId}/approval`,
        {
          approval_kind: request.approvalKind,
          action: request.action,
          resolved_by: request.resolvedBy,
          reason: request.reason,
        }
      );
      return response.data;
    } catch (error) {
      logger.error("Task approval resolution failed", error as Error, {
        taskId: request.taskId,
        approvalKind: request.approvalKind,
        action: request.action,
      });
      throw error;
    }
  }

  async sendConnectorEvent(
    connector: string,
    request: { type: string; userId: string; data: unknown }
  ): Promise<void> {
    try {
      await this.client.post(
        `/v1/connectors/${encodeURIComponent(connector)}/events`,
        {
          type: request.type,
          user_id: request.userId,
          data: request.data,
        }
      );
    } catch (error) {
      logger.error("Slack event processing failed", error as Error);
      throw error;
    }
  }

  async postLinearStatus(params: {
    issueId?: string;
    identifier?: string;
    url?: string;
    state?: string;
    taskId?: string;
    message: string;
  }): Promise<void> {
    await this.sendConnectorEvent("linear", {
      type: "frontal-code.status",
      userId: params.taskId ?? "",
      data: params,
    });
  }

  async postGraphiteStatus(params: {
    stackId?: string;
    headBranch?: string;
    baseBranch?: string;
    taskId?: string;
    message: string;
  }): Promise<void> {
    await this.sendConnectorEvent("graphite", {
      type: "frontal-code.status",
      userId: params.taskId ?? "",
      data: params,
    });
  }

  async checkSandboxStatus(): Promise<FrontalCodeSandboxResponse> {
    try {
      return await this.getSandboxStatus();
    } catch (error) {
      logger.error("Failed to check sandbox status", error as Error);
      throw error;
    }
  }
}

export default FrontalCodeApiClient;
