// Present intervals include all display stalls. Only the explicit warmup is
// excluded; no percentile trimming or gap filtering may hide missed frames.
export function presentationMetrics(events, { warmupMs = 3000, targetHz = 60 } = {}) {
  const started = events.find(e => e.event === "start");
  const submitted = events.filter(e => e.event === "submit");
  const presented = events.filter(e => e.event === "present");
  const visible = presented.filter(e => e.valid && e.atMs > 0).sort((a,b) => a.atMs-b.atMs);
  const steady = visible.filter(e => e.atMs >= warmupMs);
  const intervals = steady.slice(1).map((e,i) => e.atMs-steady[i].atMs);
  const budget = 1000/targetHz;
  const stats = values => {
    const sorted = [...values].sort((a,b)=>a-b);
    const percentile = p => sorted.length ? sorted[Math.ceil(p*sorted.length)-1] : null;
    return { count: sorted.length, p50:percentile(.5), p95:percentile(.95), p99:percentile(.99), max:sorted.at(-1) ?? null };
  };
  const byIndex = new Map(submitted.map(e=>[e.index,e]));
  const pending = submitted.filter(e => !presented.some(p => p.index === e.index));
  // An in-flight frame at deliberate process termination is not called dropped.
  // Older unmatched acquisitions, however, invalidate acceptance.
  const lastTime = Math.max(0,...events.map(e=>e.atMs ?? 0));
  const stalePending = pending.filter(e=>e.atMs < lastTime-250);
  const late = intervals.filter(ms=>ms > budget*1.5);
  const intervalMs = stats(intervals);
  const spanMs = steady.length > 1 ? steady.at(-1).atMs-steady[0].atMs : 0;
  const stopped=events.find(e=>e.event==="stop");
  const windowStateEvents=events.filter(e=>e.event==="window-state").sort((a,b)=>a.atMs-b.atMs);
  let key=false, active=false;
  for(const event of windowStateEvents.filter(e=>e.atMs<=warmupMs)) {
    if(event.name==="NSWindowDidBecomeKeyNotification")key=true;
    if(event.name==="NSWindowDidResignKeyNotification")key=false;
    if(event.name==="NSApplicationDidBecomeActiveNotification")active=true;
    if(event.name==="NSApplicationDidResignActiveNotification")active=false;
  }
  const foregroundContinuous=key && active && !windowStateEvents.some(e=>e.atMs>warmupMs && e.name.includes("DidResign"));
  const result = {
    clock: started?.clock, observerIncluded:started?.observerIncluded,
    targetHz, warmupMs, submitted:submitted.length, presented:visible.length,
    invalidPresentationCallbacks:presented.filter(e=>!e.valid).length,
    staleUnpresented:stalePending.length, inFlightAtStop:pending.length-stalePending.length,
    firstPresentedMs:visible[0]?.atMs ?? null,
    tailWithoutPresentationMs:stopped && visible.length ? Math.max(0,stopped.atMs-visible.at(-1).atMs) : null,
    steadySpanMs:spanMs, steadyFPS:spanMs ? intervals.length*1000/spanMs : null,
    intervalMs, lateIntervals:late.length, lateRatio:intervals.length ? late.length/intervals.length : null,
    estimatedMissedBudgetSlots:intervals.reduce((n,ms)=>n+Math.max(0,Math.round(ms/budget)-1),0),
    submittedToPresentedMs:stats(steady.map(e=>e.atMs-byIndex.get(e.index)?.atMs).filter(Number.isFinite)),
    pixels:[...new Set(submitted.map(e=>JSON.stringify(e.pixels)))].map(v=>JSON.parse(v)),
    windowStateEvents, foregroundContinuous,
    foregroundApplications:events.filter(e=>e.event==="foreground-app"),
  };
  result.continuousAnimationPassed = started?.clock === "MTLDrawable.presentedTime" &&
    foregroundContinuous && spanMs >= 10000 && intervals.length >= 300 &&
    result.invalidPresentationCallbacks === 0 && stalePending.length === 0 &&
    result.tailWithoutPresentationMs !== null && result.tailWithoutPresentationMs <= 100 &&
    intervalMs.p95 <= budget+2 && intervalMs.p99 <= budget*1.5 && intervalMs.max <= 50 && result.lateRatio <= .01;
  return result;
}
