import assert from "node:assert/strict";
import { test } from "node:test";
import { presentationMetrics } from "./presentation-metrics.mjs";
const trace = (gap = 0) => [{event:"start",clock:"MTLDrawable.presentedTime",observerIncluded:true},
  {event:"window-state",atMs:50,name:"NSWindowDidBecomeKeyNotification"},
  {event:"window-state",atMs:51,name:"NSApplicationDidBecomeActiveNotification"},
  ...Array.from({length:1200},(_,i)=>{
    const atMs=100+i*1000/60+(i>=600?gap:0);
    return [{event:"submit",index:i,atMs:atMs-5,pixels:[1920,1280]},
      {event:"present",index:i,atMs,valid:true}];
  }).flat(),{event:"stop",atMs:100+1199*1000/60+gap+5}];
test("accepts a sustained presentation clock and preserves a display stall",()=>{
  assert.equal(presentationMetrics(trace()).continuousAnimationPassed,true);
  const stalled=presentationMetrics(trace(200));
  assert.equal(stalled.continuousAnimationPassed,false);
  assert.ok(stalled.intervalMs.max>200);
  assert.ok(stalled.estimatedMissedBudgetSlots>=12);
});
test("missing and zero-time presentations cannot certify smooth animation",()=>{
  assert.equal(presentationMetrics([]).continuousAnimationPassed,false);
  const events=trace();
  events.find(e=>e.event==="present" && e.index===600).valid=false;
  assert.equal(presentationMetrics(events).continuousAnimationPassed,false);
  const dropped=trace().filter(e=>e.event!=="present" || e.index!==600);
  assert.equal(presentationMetrics(dropped).staleUnpresented,1);
  assert.equal(presentationMetrics(dropped).continuousAnimationPassed,false);
});
test("a long silent tail cannot be hidden by clean earlier intervals",()=>{
  const events=trace();
  events.at(-1).atMs+=1000;
  const result=presentationMetrics(events);
  assert.equal(result.continuousAnimationPassed,false);
  assert.equal(result.tailWithoutPresentationMs,1005);
});
test("background presentation and missing foreground observations cannot pass",()=>{
  const events=trace();
  events.push({event:"window-state",atMs:10000,name:"NSApplicationDidResignActiveNotification"});
  assert.equal(presentationMetrics(events).continuousAnimationPassed,false);
  assert.equal(presentationMetrics(trace().filter(e=>e.event!=="window-state")).continuousAnimationPassed,false);
  const background=trace();
  background.push({event:"window-state",atMs:1000,name:"NSWindowDidResignKeyNotification"});
  assert.equal(presentationMetrics(background).foregroundContinuous,false);
});
