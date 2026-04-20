import { createSession } from "./session";

export interface SessionContext {
  userId: string;
  tenantId: string;
}

export type SessionState = "active" | "expired";

export class SessionService {
  constructor(private readonly ctx: SessionContext) {}

  invalidateSession(sessionId: string) {
    return createSession(sessionId, this.ctx.userId);
  }
}

export const invalidateLater = (sessionId: string) =>
  createSession(sessionId, "queued");
