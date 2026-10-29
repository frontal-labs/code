// Frontal Code hosted events over WebSocket. Owns the connection, reconnection, and message parsing;
// each parsed event is handed to a `FrontalCodeEventRouter` (`events.ts`) for routing.
import WebSocket from "ws";
import { FrontalCodeEventRouter, type TrackedTaskHandler } from "./events";
import { logger } from "./log";
import type {
  FrontalCodeEventEnvelope,
  FrontalCodeEventStreamQuery,
  FrontalCodeTrackedTask,
} from "./types";

type FrontalCodeEventsUrlBuilder = (query?: FrontalCodeEventStreamQuery) => string;
type FrontalCodeEventsHeadersBuilder = () => Record<string, string> | undefined;

const RECONNECT_DELAY_MS = 2_000;

export class FrontalCodeEventsClient {
  private readonly urlBuilder: FrontalCodeEventsUrlBuilder;
  private readonly headersBuilder: FrontalCodeEventsHeadersBuilder;
  private readonly router = new FrontalCodeEventRouter();
  private socket?: WebSocket;
  private reconnectTimer?: NodeJS.Timeout;
  private shouldReconnect = false;
  private restartRequested = false;

  constructor(
    urlBuilder: FrontalCodeEventsUrlBuilder,
    headersBuilder: FrontalCodeEventsHeadersBuilder = () => undefined,
  ) {
    this.urlBuilder = urlBuilder;
    this.headersBuilder = headersBuilder;
  }

  async connect(): Promise<void> {
    this.shouldReconnect = true;
    await this.openSocket();
  }

  async disconnect(): Promise<void> {
    this.shouldReconnect = false;
    this.restartRequested = false;
    if (this.reconnectTimer) {
      clearTimeout(this.reconnectTimer);
      this.reconnectTimer = undefined;
    }

    const socket = this.socket;
    this.socket = undefined;
    if (!socket) {
      return;
    }

    await new Promise<void>((resolve) => {
      socket.once("close", () => resolve());
      socket.close();
    });
  }

  onTrackedTaskEvent(handler: TrackedTaskHandler): () => void {
    return this.router.onTrackedTaskEvent(handler);
  }

  trackTask(task: FrontalCodeTrackedTask): void {
    this.router.trackTask(task);
  }

  untrackTask(taskId: string): void {
    this.router.untrackTask(taskId);
  }

  private async openSocket(): Promise<void> {
    const nextUrl = this.buildSocketUrl();
    const headers = this.headersBuilder();
    await new Promise<void>((resolve, reject) => {
      const socket = new WebSocket(nextUrl, headers ? { headers } : undefined);
      this.socket = socket;

      socket.once("open", () => {
        logger.info("Connected to Frontal Code hosted events stream", {
          url: nextUrl,
        });
        resolve();
      });

      socket.once("error", (error) => {
        logger.error("Frontal Code hosted events stream error", error as Error);
        reject(error);
      });

      socket.on("message", (data) => {
        this.handleMessage(data.toString());
      });

      socket.on("close", () => {
        logger.warn("Frontal Code hosted events stream disconnected");
        this.socket = undefined;
        if (this.shouldReconnect) {
          if (this.restartRequested) {
            this.restartRequested = false;
            this.openSocket().catch((error) => {
              logger.error("Frontal Code hosted events stream restart failed", error as Error);
              this.scheduleReconnect();
            });
          } else {
            this.scheduleReconnect();
          }
        }
      });
    });
  }

  private buildSocketUrl(): string {
    return this.urlBuilder({
      source: "slack",
    });
  }

  private scheduleReconnect(): void {
    if (this.reconnectTimer) {
      return;
    }

    this.reconnectTimer = setTimeout(() => {
      this.reconnectTimer = undefined;
      if (!this.shouldReconnect) {
        return;
      }
      this.openSocket().catch((error) => {
        logger.error("Frontal Code hosted events reconnect failed", error as Error);
        this.scheduleReconnect();
      });
    }, RECONNECT_DELAY_MS);
  }

  private handleMessage(payload: string): void {
    let event: FrontalCodeEventEnvelope;
    try {
      event = JSON.parse(payload) as FrontalCodeEventEnvelope;
    } catch (_error) {
      logger.warn("Ignoring malformed Frontal Code hosted event payload", {
        payload,
      });
      return;
    }

    this.router.receive(event);
  }
}

export default FrontalCodeEventsClient;
