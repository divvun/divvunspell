/**
 * How much the library logs. Each level takes in the ones before it, and
 * `off` logs nothing. The default is `warn`.
 */
export type LogLevel = "off" | "error" | "warn" | "info" | "debug" | "trace";

/** One event from the library's `tracing` instrumentation. */
export interface LogEvent {
  level: Exclude<LogLevel, "off">;
  /** The Rust module the event came from, such as `divvun_fst::archive`. */
  target: string;
  message: string;
}

/** Receives every event at or above the log level. */
export type Logger = (event: LogEvent) => void;

/** The levels in the glue's numbering, where each level is its index. */
const LEVELS: readonly LogLevel[] = ["off", "error", "warn", "info", "debug", "trace"];

export function isLogLevel(value: unknown): value is LogLevel {
  return typeof value === "string" && (LEVELS as readonly string[]).includes(value);
}

export function levelNumber(level: LogLevel): number {
  return LEVELS.indexOf(level);
}

/** The level of an event from the glue. A number it does not use reads as trace. */
export function eventLevel(level: number): LogEvent["level"] {
  const name = LEVELS[level];
  return name === undefined || name === "off" ? "trace" : name;
}

/**
 * The logger in place until `setLogger` replaces it: each event goes to the
 * console method for its level. At the default level only warnings and
 * errors reach it.
 */
export const consoleLogger: Logger = ({ level, target, message }) => {
  const line = `${target}: ${message}`;
  switch (level) {
    case "error":
      console.error(line);
      break;
    case "warn":
      console.warn(line);
      break;
    case "info":
      console.info(line);
      break;
    case "debug":
    case "trace":
      console.debug(line);
      break;
  }
};
