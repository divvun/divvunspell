// The wrapper's sink for this module instance, an object with
// log(level, target, message) and panic(message), and the log level in force.
// The panic hook and the tracing subscriber must be Send and Sync, so they
// cannot hold a JavaScript object; they call these functions, which hold it
// here instead. The generated glue imports this module once, so the package
// has one sink and one level.

let sink = null;
let level = 0;

export function installSink(value, initialLevel) {
  sink = value;
  level = initialLevel;
}

export function setSinkLevel(value) {
  level = value;
}

export function sinkLevel() {
  return level;
}

export function sinkLog(eventLevel, target, message) {
  if (sink !== null) sink.log(eventLevel, target, message);
}

export function sinkPanic(message) {
  if (sink !== null) sink.panic(message);
}
