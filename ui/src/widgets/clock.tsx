import { useEffect, useState } from "react";
import type { Bridge } from "../bridge";

function localTime() {
  const now = new Date();
  const greeting = now.getHours() < 12 ? "Good morning" : now.getHours() < 18 ? "Good afternoon" : "Good evening";
  return {
    datetime: now.toISOString(),
    greeting,
    value: new Intl.DateTimeFormat([], { hour: "2-digit", minute: "2-digit" }).format(now),
  };
}

export function ClockWidget(_props: { bridge: Bridge }) {
  const [clock, setClock] = useState(localTime);

  useEffect(() => {
    const timer = window.setInterval(() => setClock(localTime()), 15_000);
    return () => window.clearInterval(timer);
  }, []);

  return (
    <div className="clock-face">
      <time dateTime={clock.datetime}>{clock.value}</time>
      <p>{clock.greeting}</p>
    </div>
  );
}
