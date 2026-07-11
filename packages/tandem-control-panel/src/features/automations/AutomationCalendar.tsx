import type { DateClickArg } from "@fullcalendar/interaction";
import type FullCalendar from "@fullcalendar/react";
import type { DatesSetArg, EventClickArg, EventContentArg, MoreLinkArg } from "@fullcalendar/core";
import { useEffect, useMemo, useRef, useState } from "react";
import { prepareFullCalendarStyleSheet } from "./fullCalendarTauriCompat";
import { detectBrowserTimezone } from "./timezone";

type CalendarRange = {
  startMs: number;
  endMs: number;
};

type AutomationCalendarProps = {
  events: any[];
  onRangeChange: (range: CalendarRange) => void;
  onOpenAutomation: (automation: any) => void;
};

type FullCalendarRuntime = {
  FullCalendar: typeof import("@fullcalendar/react").default;
  interactionPlugin: typeof import("@fullcalendar/interaction").default;
  timeGridPlugin: typeof import("@fullcalendar/timegrid").default;
};

const DEFAULT_DAY_SCROLL_TIME = "09:00:00";
const CALENDAR_BLOCK_MS = 30 * 60 * 1000;
const CALENDAR_DISPLAY_TIMEZONE = detectBrowserTimezone();

function pad2(value: number) {
  return String(value).padStart(2, "0");
}

function startOfCalendarMinute(date: Date) {
  return new Date(
    date.getFullYear(),
    date.getMonth(),
    date.getDate(),
    date.getHours(),
    date.getMinutes(),
    0,
    0
  );
}

function toScrollTime(date: Date) {
  return `${pad2(date.getHours())}:${pad2(date.getMinutes())}:00`;
}

function timeParts(scrollTime: string) {
  const [hourText, minuteText] = String(scrollTime || DEFAULT_DAY_SCROLL_TIME).split(":");
  return {
    hour: Number.parseInt(hourText || "9", 10) || 9,
    minute: Number.parseInt(minuteText || "0", 10) || 0,
  };
}

function floorToCalendarBlock(date: Date) {
  return new Date(Math.floor(date.getTime() / CALENDAR_BLOCK_MS) * CALENDAR_BLOCK_MS);
}

function formatFocusedBlock(date: Date | null) {
  if (!date) return "";
  const start = floorToCalendarBlock(date);
  const end = new Date(start.getTime() + CALENDAR_BLOCK_MS);
  return `${new Intl.DateTimeFormat("en-US", {
    weekday: "short",
    month: "short",
    day: "numeric",
    hour: "numeric",
    minute: "2-digit",
    timeZone: CALENDAR_DISPLAY_TIMEZONE,
  }).format(start)} - ${new Intl.DateTimeFormat("en-US", {
    hour: "numeric",
    minute: "2-digit",
    timeZone: CALENDAR_DISPLAY_TIMEZONE,
    timeZoneName: "short",
  }).format(end)}`;
}

function formatFocusedBlockHeading(date: Date | null) {
  if (!date) return "";
  const start = floorToCalendarBlock(date);
  const end = new Date(start.getTime() + CALENDAR_BLOCK_MS);
  const formatter = new Intl.DateTimeFormat("en-US", {
    weekday: "short",
    month: "short",
    day: "numeric",
    hour: "numeric",
    minute: "2-digit",
    timeZone: CALENDAR_DISPLAY_TIMEZONE,
  });
  const endFormatter = new Intl.DateTimeFormat("en-US", {
    hour: "numeric",
    minute: "2-digit",
    timeZone: CALENDAR_DISPLAY_TIMEZONE,
    timeZoneName: "short",
  });
  return `${formatter.format(start)} - ${endFormatter.format(end)}`;
}

function normalizeEventStart(value: unknown) {
  if (value instanceof Date) return value;
  const date = new Date(String(value || ""));
  return Number.isNaN(date.getTime()) ? null : date;
}

function familyLabel(value: string) {
  const normalized = String(value || "")
    .trim()
    .toLowerCase();
  if (normalized === "v2") return "workflow";
  if (normalized === "legacy") return "routine";
  return normalized || "task";
}

function familyToneClasses(value: string) {
  const normalized = String(value || "")
    .trim()
    .toLowerCase();
  if (normalized === "v2") {
    return {
      shell: "border-amber-400/40 bg-amber-400/12 text-amber-100",
      chip: "border-amber-400/30 bg-amber-400/12 text-amber-100",
      dot: "bg-amber-300",
    };
  }
  if (normalized === "legacy") {
    return {
      shell: "border-sky-400/30 bg-sky-400/10 text-sky-100",
      chip: "border-sky-400/30 bg-sky-400/10 text-sky-100",
      dot: "bg-sky-300",
    };
  }
  return {
    shell: "border-slate-500/40 bg-slate-500/10 text-slate-100",
    chip: "border-slate-500/40 bg-slate-500/10 text-slate-100",
    dot: "bg-slate-300",
  };
}

function displayStatusLabel(value: string) {
  const normalized = String(value || "")
    .trim()
    .toLowerCase();
  if (normalized === "in_progress") return "running";
  if (normalized === "done") return "completed";
  if (normalized === "error") return "failed";
  return normalized || "active";
}

function statusToneClasses(value: string) {
  const normalized = String(value || "")
    .trim()
    .toLowerCase();
  if (
    normalized === "active" ||
    normalized === "running" ||
    normalized === "in_progress" ||
    normalized === "completed" ||
    normalized === "done"
  ) {
    return {
      shell: "border-emerald-400/35 bg-emerald-400/12 text-emerald-100",
      chip: "border-emerald-400/30 bg-emerald-400/12 text-emerald-100",
      dot: "bg-emerald-300",
    };
  }
  if (normalized === "paused") {
    return {
      shell: "border-slate-500/40 bg-slate-500/10 text-slate-100",
      chip: "border-slate-500/40 bg-slate-500/10 text-slate-100",
      dot: "bg-slate-300",
    };
  }
  if (normalized === "blocked" || normalized === "queued" || normalized === "awaiting_approval") {
    return {
      shell: "border-amber-400/35 bg-amber-400/12 text-amber-100",
      chip: "border-amber-400/30 bg-amber-400/12 text-amber-100",
      dot: "bg-amber-300",
    };
  }
  if (normalized === "failed" || normalized === "error" || normalized === "stalled") {
    return {
      shell: "border-rose-400/35 bg-rose-400/12 text-rose-100",
      chip: "border-rose-400/30 bg-rose-400/12 text-rose-100",
      dot: "bg-rose-300",
    };
  }
  return {
    shell: "border-slate-500/40 bg-slate-500/10 text-slate-100",
    chip: "border-slate-500/40 bg-slate-500/10 text-slate-100",
    dot: "bg-slate-300",
  };
}

export function AutomationCalendar({
  events,
  onRangeChange,
  onOpenAutomation,
}: AutomationCalendarProps) {
  const calendarRef = useRef<FullCalendar | null>(null);
  const detailPanelRef = useRef<HTMLDivElement | null>(null);
  const pendingScrollTimeRef = useRef("");
  const pendingFocusDateRef = useRef<Date | null>(null);
  const [currentView, setCurrentView] = useState("timeGridWeek");
  const [focusedSlot, setFocusedSlot] = useState<Date | null>(null);
  const [fullCalendarRuntime, setFullCalendarRuntime] = useState<FullCalendarRuntime | null>(null);
  const [fullCalendarLoadError, setFullCalendarLoadError] = useState("");
  const FullCalendarView = fullCalendarRuntime?.FullCalendar;

  useEffect(() => {
    let cancelled = false;

    async function loadFullCalendar() {
      try {
        await prepareFullCalendarStyleSheet();
        if (cancelled) return;
        const [reactModule, interactionModule, timeGridModule] = await Promise.all([
          import("@fullcalendar/react"),
          import("@fullcalendar/interaction"),
          import("@fullcalendar/timegrid"),
        ]);
        if (cancelled) return;
        setFullCalendarRuntime({
          FullCalendar: reactModule.default,
          interactionPlugin: interactionModule.default,
          timeGridPlugin: timeGridModule.default,
        });
        setFullCalendarLoadError("");
      } catch (error) {
        if (cancelled) return;
        setFullCalendarLoadError(error instanceof Error ? error.message : String(error));
      }
    }

    void loadFullCalendar();

    return () => {
      cancelled = true;
    };
  }, []);

  const focusedBlockLabel = useMemo(() => formatFocusedBlock(focusedSlot), [focusedSlot]);
  const focusedBlockHeading = useMemo(() => formatFocusedBlockHeading(focusedSlot), [focusedSlot]);
  const groupedBlockEvents = useMemo(() => {
    const blocks = new Map<
      number,
      {
        event: any;
        start: Date;
        blockStartMs: number;
      }[]
    >();
    for (const event of events) {
      const start = normalizeEventStart(event?.start);
      if (!start) continue;
      const blockStart = floorToCalendarBlock(start);
      const blockStartMs = blockStart.getTime();
      const bucket = blocks.get(blockStartMs) || [];
      bucket.push({ event, start, blockStartMs });
      blocks.set(blockStartMs, bucket);
    }
    return Array.from(blocks.entries())
      .sort((left, right) => left[0] - right[0])
      .map(([blockStartMs, bucket]) => {
        const items = bucket.slice().sort((left, right) => {
          const leftStart = left.start?.getTime() || 0;
          const rightStart = right.start?.getTime() || 0;
          if (leftStart !== rightStart) return leftStart - rightStart;
          return String(left.event?.title || "").localeCompare(String(right.event?.title || ""));
        });
        const blockStart = new Date(blockStartMs);
        const blockEnd = new Date(blockStartMs + CALENDAR_BLOCK_MS);
        const familyCounts: Record<string, number> = {};
        const statusCounts: Record<string, number> = {};
        const previewItems = items.slice(0, 3).map(({ event }) => {
          const title = String(
            event?.title ||
              event?.extendedProps?.automation?.name ||
              event?.extendedProps?.automation?.automation_id ||
              "Automation"
          ).trim();
          const family = String(event?.extendedProps?.family || "legacy").trim();
          const status = displayStatusLabel(
            String(event?.extendedProps?.status || "active").trim()
          );
          const scheduleLabel = String(event?.extendedProps?.scheduleLabel || "").trim();
          familyCounts[family] = (familyCounts[family] || 0) + 1;
          statusCounts[status] = (statusCounts[status] || 0) + 1;
          return {
            title,
            family,
            status,
            scheduleLabel,
          };
        });
        const dominantFamily =
          Object.entries(familyCounts).sort((left, right) => {
            if (left[1] !== right[1]) return right[1] - left[1];
            return left[0].localeCompare(right[0]);
          })[0]?.[0] || "legacy";
        const dominantStatus =
          Object.entries(statusCounts).sort((left, right) => {
            if (left[1] !== right[1]) return right[1] - left[1];
            return left[0].localeCompare(right[0]);
          })[0]?.[0] || "active";
        const previewTitles = previewItems.map((item) => item.title).filter(Boolean);
        const previewText =
          previewTitles[0] ||
          String(
            items[0]?.event?.title ||
              items[0]?.event?.extendedProps?.automation?.name ||
              items[0]?.event?.extendedProps?.automation?.automation_id ||
              "Automation"
          ).trim();
        const overflowCount = Math.max(0, items.length - previewItems.length);
        const title = `${items.length} ${items.length === 1 ? "task" : "tasks"}`;
        const hint = [
          `${formatFocusedBlock(blockStart)} · ${title}`,
          ...items.slice(0, 4).map(({ event }) => {
            const name = String(
              event?.title ||
                event?.extendedProps?.automation?.name ||
                event?.extendedProps?.automation?.automation_id ||
                "Automation"
            ).trim();
            const family = String(event?.extendedProps?.family || "legacy").trim();
            const status = String(event?.extendedProps?.status || "active").trim();
            const scheduleLabel = String(event?.extendedProps?.scheduleLabel || "").trim();
            return [name, family, status, scheduleLabel].filter(Boolean).join(" · ");
          }),
          items.length > 4 ? `+${items.length - 4} more` : "",
        ]
          .filter(Boolean)
          .join("\n");
        return {
          blockStartMs,
          blockEndMs: blockEnd.getTime(),
          start: blockStart,
          end: blockEnd,
          items,
          count: items.length,
          title,
          hint,
          previewItems,
          previewText,
          overflowCount,
          dominantFamily,
          dominantStatus,
          familyCounts,
          statusCounts,
        };
      });
  }, [events]);
  const focusedBlockItems = useMemo(() => {
    if (!focusedSlot) return [];
    return (
      groupedBlockEvents.find(({ blockStartMs }) => blockStartMs === focusedSlot.getTime())
        ?.items || []
    );
  }, [focusedSlot, groupedBlockEvents]);

  const syncFocusedSlot = (date: Date | null) => {
    if (!date) {
      setFocusedSlot(null);
      pendingScrollTimeRef.current = "";
      pendingFocusDateRef.current = null;
      return;
    }
    const normalized = startOfCalendarMinute(date);
    setFocusedSlot(normalized);
    pendingFocusDateRef.current = normalized;
    pendingScrollTimeRef.current = toScrollTime(normalized);
  };

  const revealFocusedBlockPanel = () => {
    window.requestAnimationFrame(() => {
      detailPanelRef.current?.scrollIntoView({
        behavior: "smooth",
        block: "start",
      });
    });
  };

  const selectBlockAtTime = (date: Date) => {
    const normalized = startOfCalendarMinute(date);
    const blockStart = floorToCalendarBlock(normalized);
    syncFocusedSlot(blockStart);
    revealFocusedBlockPanel();
  };

  const openDayViewAtTime = (date: Date) => {
    const normalized = startOfCalendarMinute(date);
    const blockStart = floorToCalendarBlock(normalized);
    syncFocusedSlot(blockStart);
    const api = calendarRef.current?.getApi();
    if (!api) return;
    pendingFocusDateRef.current = blockStart;
    api.changeView("timeGridDay");
    api.gotoDate(blockStart);
    window.requestAnimationFrame(() => {
      window.requestAnimationFrame(() => {
        const nextApi = calendarRef.current?.getApi();
        if (!nextApi || !pendingFocusDateRef.current) return;
        nextApi.gotoDate(pendingFocusDateRef.current);
        if (pendingScrollTimeRef.current) {
          nextApi.scrollToTime(pendingScrollTimeRef.current);
        }
      });
    });
  };

  const handleDatesSet = (arg: DatesSetArg) => {
    setCurrentView(arg.view.type);
    onRangeChange({
      startMs: arg.start.getTime(),
      endMs: arg.end.getTime(),
    });
    if (arg.view.type !== "timeGridDay" || !pendingScrollTimeRef.current) return;
    window.requestAnimationFrame(() => {
      const api = calendarRef.current?.getApi();
      if (!api) return;
      if (pendingFocusDateRef.current) {
        api.gotoDate(pendingFocusDateRef.current);
      }
      api.scrollToTime(pendingScrollTimeRef.current);
      pendingScrollTimeRef.current = "";
    });
  };

  const handleEventClick = (arg: EventClickArg) => {
    arg.jsEvent?.preventDefault?.();
    const start = arg.event?.start || null;
    if (start) {
      selectBlockAtTime(start);
    }
  };

  const handleDateClick = (arg: DateClickArg) => {
    selectBlockAtTime(arg.date);
  };

  const handleMoreLinkClick = (arg: MoreLinkArg) => {
    arg.jsEvent?.preventDefault?.();
    selectBlockAtTime(arg.date);
  };

  const renderEventContent = (arg: EventContentArg) => {
    const count = Number(arg?.event?.extendedProps?.count || 0) || 0;
    const selected =
      focusedSlot?.getTime() === Number(arg?.event?.extendedProps?.blockStartMs || 0);
    const familyCounts = (arg?.event?.extendedProps?.familyCounts || {}) as Record<string, number>;
    const statusCounts = (arg?.event?.extendedProps?.statusCounts || {}) as Record<string, number>;
    const previewText = String(arg?.event?.extendedProps?.previewText || "").trim();
    const overflowCount = Number(arg?.event?.extendedProps?.overflowCount || 0) || 0;
    const familySwatches = Object.entries(familyCounts)
      .sort((left, right) => {
        if (left[1] !== right[1]) return right[1] - left[1];
        return left[0].localeCompare(right[0]);
      })
      .slice(0, 3)
      .map(([family, value]) => ({
        family,
        count: value,
        tone: familyToneClasses(family),
      }));
    const dominantStatus =
      Object.entries(statusCounts).sort((left, right) => {
        if (left[1] !== right[1]) return right[1] - left[1];
        return left[0].localeCompare(right[0]);
      })[0]?.[0] || "active";
    const statusTone = statusToneClasses(dominantStatus);
    return (
      <div
        className={`group relative flex h-full min-h-0 items-center overflow-hidden border px-2 py-1 text-xs shadow-sm transition-colors ${
          selected
            ? "border-amber-400/80 bg-amber-400/14 text-amber-100"
            : "border-slate-700/60 bg-slate-950/86 text-slate-100"
        }`}
      >
        <div
          className={`absolute inset-y-0 left-0 w-0.5 ${
            selected
              ? "bg-amber-300"
              : familySwatches[0]?.tone?.dot || statusTone.dot || "bg-slate-500"
          }`}
        />
        <div className="relative flex min-w-0 w-full items-center gap-2 pl-1">
          <div className="flex shrink-0 items-center gap-1">
            {familySwatches.length ? (
              familySwatches.map((swatch, index) => (
                <span
                  key={`${swatch.family}-${index}`}
                  className={`inline-flex h-2.5 rounded-full border ${
                    selected ? "border-amber-300/80 bg-amber-300/35" : swatch.tone.chip
                  }`}
                  style={{
                    width: index === 0 ? "1.4rem" : index === 1 ? "0.95rem" : "0.7rem",
                  }}
                  title={`${familyLabel(swatch.family)} · ${swatch.count}`}
                  aria-hidden="true"
                />
              ))
            ) : (
              <span
                className={`inline-flex h-2.5 w-3 rounded-full border ${
                  selected ? "border-amber-300/80 bg-amber-300/35" : statusTone.chip
                }`}
                title={dominantStatus}
                aria-hidden="true"
              />
            )}
          </div>
          <span className="min-w-0 flex-1 truncate tcp-text-caption font-medium leading-none text-slate-100">
            {previewText}
          </span>
          {overflowCount > 0 ? (
            <span className="inline-flex shrink-0 items-center rounded-full border border-slate-600/70 bg-slate-950/80 px-1.5 py-0.5 text-micro font-semibold uppercase tracking-[0.18em] text-slate-200">
              +{overflowCount}
            </span>
          ) : null}
          <span
            className={`inline-flex shrink-0 items-center rounded-md border px-1.5 py-0.5 tcp-text-caption font-semibold tabular-nums ${
              selected
                ? "border-amber-300/70 bg-amber-300/18 text-amber-50"
                : "border-slate-600/70 bg-slate-950/70 text-slate-100"
            }`}
          >
            {count}
          </span>
          <span
            className={`inline-flex shrink-0 h-2 w-2 rounded-full border ${
              selected ? "border-amber-200/80 bg-amber-300" : statusTone.dot + " border-slate-900/0"
            }`}
            title={dominantStatus}
            aria-hidden="true"
          />
        </div>
      </div>
    );
  };

  return (
    <div className="grid gap-3">
      <div className="flex flex-wrap items-start justify-between gap-2">
        <div className="grid gap-1">
          <p className="text-xs font-medium uppercase tracking-wide text-slate-500">
            Scheduled automations
          </p>
          <div className="tcp-subtle text-xs">
            Automations are grouped into 30-minute blocks so crowded times stay readable. Hover a
            block for a preview, click it to inspect the workflows inside, and use the block details
            below to open individual editors. The calendar is shown in your local time (
            {CALENDAR_DISPLAY_TIMEZONE}). Cron and interval automations are shown here.
          </div>
        </div>
        <div className="flex flex-wrap items-center justify-end gap-2">
          {focusedBlockLabel ? <span className="tcp-badge-info">{focusedBlockLabel}</span> : null}
          <span className="tcp-badge-info">
            {groupedBlockEvents.length} scheduled block
            {groupedBlockEvents.length === 1 ? "" : "s"}
          </span>
        </div>
      </div>
      <div className="overflow-hidden rounded-2xl border border-slate-800/80 bg-slate-950/35 p-2">
        {FullCalendarView && fullCalendarRuntime ? (
          <FullCalendarView
            ref={calendarRef}
            plugins={[fullCalendarRuntime.timeGridPlugin, fullCalendarRuntime.interactionPlugin]}
            initialView="timeGridWeek"
            timeZone="local"
            firstDay={0}
            height="auto"
            expandRows
            nowIndicator
            editable={false}
            eventStartEditable={false}
            eventDurationEditable={false}
            eventOverlap
            slotEventOverlap
            allDaySlot={false}
            slotMinTime="00:00:00"
            slotMaxTime="24:00:00"
            scrollTimeReset={false}
            stickyHeaderDates
            navLinks
            buttonIcons={false}
            headerToolbar={{
              left: "prev,next today",
              center: "title",
              right: "timeGridWeek,timeGridDay",
            }}
            buttonText={{
              prev: "previous",
              next: "next",
              timeGridWeek: "week",
              timeGridDay: "day",
              today: "today",
            }}
            views={{
              timeGridWeek: {
                eventMaxStack: 1,
              },
              timeGridDay: {
                slotDuration: "00:30:00",
                slotLabelInterval: "00:30:00",
              },
            }}
            events={groupedBlockEvents}
            datesSet={handleDatesSet}
            navLinkDayClick={(date) => {
              const fallbackTime = focusedSlot
                ? toScrollTime(focusedSlot)
                : DEFAULT_DAY_SCROLL_TIME;
              const { hour, minute } = timeParts(fallbackTime);
              openDayViewAtTime(
                new Date(date.getFullYear(), date.getMonth(), date.getDate(), hour, minute, 0, 0)
              );
            }}
            dateClick={handleDateClick}
            moreLinkClick={handleMoreLinkClick}
            moreLinkClassNames={() => ["tcp-calendar-more-link"]}
            moreLinkContent={(arg) => <span>+{arg.num} more</span>}
            eventClick={handleEventClick}
            eventContent={renderEventContent}
            eventDidMount={(arg) => {
              const hint = String(arg?.event?.extendedProps?.hint || "").trim();
              if (hint) {
                arg.el.title = hint;
                arg.el.setAttribute("aria-label", hint);
              }
            }}
            slotLaneClassNames={(arg) => {
              if (currentView !== "timeGridDay" || !focusedSlot || !arg.date) return [];
              const slotTime = arg.date.getTime();
              const blockStart = focusedSlot.getTime();
              const blockEnd = blockStart + CALENDAR_BLOCK_MS;
              return slotTime >= blockStart && slotTime < blockEnd
                ? ["tcp-calendar-slot-focus"]
                : [];
            }}
            eventClassNames={() => ["tcp-calendar-event"]}
          />
        ) : (
          <div className="grid min-h-[24rem] place-items-center px-4 text-center text-sm text-slate-400">
            {fullCalendarLoadError
              ? `Calendar failed to load: ${fullCalendarLoadError}`
              : "Loading calendar..."}
          </div>
        )}
      </div>
      <div
        ref={detailPanelRef}
        className="rounded-2xl border border-slate-800/80 bg-slate-950/45 p-3 scroll-mt-24"
      >
        <div className="flex flex-wrap items-start justify-between gap-2">
          <div className="grid gap-1">
            <p className="text-xs font-medium uppercase tracking-wide text-slate-500">
              30-minute block drill-down
            </p>
            <div className="tcp-subtle text-xs">
              Click a block or a day slot to inspect the automations in that local window, then
              click any workflow in the list to open its editor.
            </div>
          </div>
          <div className="flex flex-wrap items-center gap-2">
            {focusedBlockLabel ? <span className="tcp-badge-info">{focusedBlockLabel}</span> : null}
            {focusedSlot ? (
              <button
                type="button"
                className="tcp-btn h-7 px-2.5 tcp-text-caption"
                onClick={() => syncFocusedSlot(null)}
              >
                Clear
              </button>
            ) : null}
          </div>
        </div>
        {focusedSlot ? (
          <div className="mt-3 grid gap-2">
            <div className="text-sm font-medium text-slate-200">
              {focusedBlockHeading || "Selected block"}
              {focusedBlockItems.length
                ? ` · ${focusedBlockItems.length} item${focusedBlockItems.length === 1 ? "" : "s"}`
                : ""}
            </div>
            {focusedBlockItems.length ? (
              <div className="grid gap-2 max-h-80 overflow-auto pr-1">
                {focusedBlockItems.map(({ event, start }) => {
                  const automation = event?.extendedProps?.automation;
                  const title = String(
                    event?.title || automation?.name || automation?.automation_id || "Automation"
                  ).trim();
                  const scheduleLabel = String(event?.extendedProps?.scheduleLabel || "").trim();
                  const status =
                    String(event?.extendedProps?.status || "active").trim() || "active";
                  const family = String(event?.extendedProps?.family || "legacy").trim();
                  const timeLabel = start
                    ? new Intl.DateTimeFormat("en-US", {
                        hour: "numeric",
                        minute: "2-digit",
                        timeZone: CALENDAR_DISPLAY_TIMEZONE,
                        timeZoneName: "short",
                      }).format(start)
                    : "time unavailable";
                  return (
                    <button
                      key={String(event?.id || `${title}-${timeLabel}`)}
                      type="button"
                      className="grid gap-1 rounded-xl border border-slate-700/60 bg-slate-900/60 px-3 py-2 text-left transition-colors hover:border-amber-400/60 hover:bg-amber-400/10"
                      onClick={() => automation && onOpenAutomation(automation)}
                    >
                      <div className="flex flex-wrap items-center justify-between gap-2">
                        <div className="min-w-0">
                          <div className="truncate text-sm font-medium text-slate-100">{title}</div>
                          <div className="text-xs text-slate-400">
                            {timeLabel} · {family === "v2" ? "workflow" : "routine"}
                          </div>
                        </div>
                        <div className="flex flex-wrap items-center gap-1">
                          <span className="tcp-badge-info">{family}</span>
                          <span className="tcp-badge-info">{status}</span>
                        </div>
                      </div>
                      {scheduleLabel ? (
                        <div className="text-xs text-slate-500">{scheduleLabel}</div>
                      ) : null}
                    </button>
                  );
                })}
              </div>
            ) : (
              <div className="rounded-xl border border-dashed border-slate-700/60 bg-slate-900/30 p-3 text-xs text-slate-500">
                No automations are scheduled in this block.
              </div>
            )}
          </div>
        ) : (
          <div className="mt-3 rounded-xl border border-dashed border-slate-700/60 bg-slate-900/30 p-3 text-xs text-slate-500">
            Select any slot or click a block to drill into the exact automations.
          </div>
        )}
      </div>
    </div>
  );
}
