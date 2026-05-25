import interactionPlugin, { type DateClickArg } from "@fullcalendar/interaction";
import type {
  DatesSetArg,
  EventClickArg,
  EventContentArg,
  EventDropArg,
  MoreLinkArg,
} from "@fullcalendar/core";
import FullCalendar from "@fullcalendar/react";
import timeGridPlugin from "@fullcalendar/timegrid";
import { useMemo, useRef, useState } from "react";

import type {
  AutomationCalendarEvent,
  CalendarRange,
} from "@/components/agent-automation/automationCalendarUtils";
import type { AutomationV2Spec } from "@/lib/tauri";

type AutomationCalendarProps = {
  events: AutomationCalendarEvent[];
  onRangeChange: (range: CalendarRange) => void;
  onOpenAutomation: (automationId: string) => void | Promise<void>;
  onEventDrop: (info: EventDropArg) => void | Promise<void>;
};

const DEFAULT_DAY_SCROLL_TIME = "09:00:00";

function pad2(value: number) {
  return String(value).padStart(2, "0");
}

function startOfUtcMinute(date: Date) {
  return new Date(
    Date.UTC(
      date.getUTCFullYear(),
      date.getUTCMonth(),
      date.getUTCDate(),
      date.getUTCHours(),
      date.getUTCMinutes(),
      0,
      0
    )
  );
}

function toScrollTime(date: Date) {
  return `${pad2(date.getUTCHours())}:${pad2(date.getUTCMinutes())}:00`;
}

function timeParts(scrollTime: string) {
  const [hourText, minuteText] = String(scrollTime || DEFAULT_DAY_SCROLL_TIME).split(":");
  return {
    hour: Number.parseInt(hourText || "9", 10) || 9,
    minute: Number.parseInt(minuteText || "0", 10) || 0,
  };
}

function formatFocusedSlot(date: Date | null) {
  if (!date) return "";
  return `${new Intl.DateTimeFormat("en-US", {
    weekday: "short",
    month: "short",
    day: "numeric",
    hour: "numeric",
    minute: "2-digit",
    timeZone: "UTC",
  }).format(date)} UTC`;
}

function formatHourLabel(date: Date | null) {
  if (!date) return "";
  const end = new Date(date.getTime() + 60 * 60 * 1000);
  const formatter = new Intl.DateTimeFormat("en-US", {
    weekday: "short",
    month: "short",
    day: "numeric",
    hour: "numeric",
    minute: "2-digit",
    timeZone: "UTC",
  });
  const endFormatter = new Intl.DateTimeFormat("en-US", {
    hour: "numeric",
    minute: "2-digit",
    timeZone: "UTC",
  });
  return `${formatter.format(date)} - ${endFormatter.format(end)} UTC`;
}

function sameUtcHour(left: Date | null | undefined, right: Date | null | undefined) {
  if (!left || !right) return false;
  return (
    left.getUTCFullYear() === right.getUTCFullYear() &&
    left.getUTCMonth() === right.getUTCMonth() &&
    left.getUTCDate() === right.getUTCDate() &&
    left.getUTCHours() === right.getUTCHours()
  );
}

function normalizeEventStart(value: unknown) {
  if (value instanceof Date) return value;
  const date = new Date(String(value || ""));
  return Number.isNaN(date.getTime()) ? null : date;
}

function eventStatusClasses(status: string) {
  if (status === "paused") return "text-yellow-300";
  if (status === "draft") return "text-slate-300";
  return "text-emerald-300";
}

export function AutomationCalendar({
  events,
  onRangeChange,
  onOpenAutomation,
  onEventDrop,
}: AutomationCalendarProps) {
  const calendarRef = useRef<FullCalendar | null>(null);
  const pendingScrollTimeRef = useRef("");
  const [currentView, setCurrentView] = useState("timeGridWeek");
  const [focusedSlot, setFocusedSlot] = useState<Date | null>(null);

  const focusedSlotLabel = useMemo(() => formatFocusedSlot(focusedSlot), [focusedSlot]);
  const focusedHourLabel = useMemo(() => formatHourLabel(focusedSlot), [focusedSlot]);
  const focusedHourEvents = useMemo(() => {
    if (!focusedSlot) return [];
    return events
      .map((event) => ({
        event,
        start: normalizeEventStart(event?.start),
      }))
      .filter(({ start }) => sameUtcHour(start, focusedSlot))
      .sort((left, right) => {
        const leftStart = left.start?.getTime() || 0;
        const rightStart = right.start?.getTime() || 0;
        return leftStart - rightStart;
      })
      .map(({ event, start }) => ({ event, start }));
  }, [events, focusedSlot]);

  const syncFocusedSlot = (date: Date | null) => {
    if (!date) {
      setFocusedSlot(null);
      pendingScrollTimeRef.current = "";
      return;
    }
    const normalized = startOfUtcMinute(date);
    setFocusedSlot(normalized);
    pendingScrollTimeRef.current = toScrollTime(normalized);
  };

  const focusDayAtTime = (date: Date) => {
    const normalized = startOfUtcMinute(date);
    syncFocusedSlot(normalized);
    calendarRef.current?.getApi().changeView("timeGridDay", normalized);
  };

  const handleDatesSet = (arg: DatesSetArg) => {
    setCurrentView(arg.view.type);
    onRangeChange({
      startMs: arg.start.getTime(),
      endMs: arg.end.getTime(),
    });
    if (arg.view.type !== "timeGridDay" || !pendingScrollTimeRef.current) return;
    window.requestAnimationFrame(() => {
      calendarRef.current?.getApi().scrollToTime(pendingScrollTimeRef.current);
      pendingScrollTimeRef.current = "";
    });
  };

  const handleEventClick = (arg: EventClickArg) => {
    arg.jsEvent?.preventDefault?.();
    const automation = arg.event?.extendedProps?.automation as AutomationV2Spec | undefined;
    const automationId = String(automation?.automation_id || "").trim();
    if (automationId) void onOpenAutomation(automationId);
  };

  const handleDateClick = (arg: DateClickArg) => {
    focusDayAtTime(arg.date);
  };

  const handleMoreLinkClick = (arg: MoreLinkArg) => {
    arg.jsEvent?.preventDefault?.();
    focusDayAtTime(arg.date);
  };

  const renderEventContent = (arg: EventContentArg) => {
    const status = String(arg.event.extendedProps?.status || "active").trim() || "active";
    const scheduleLabel = String(arg.event.extendedProps?.scheduleLabel || "").trim();
    return (
      <div className="desktop-automation-calendar-event">
        <div className="flex items-center justify-between gap-2">
          <span className="truncate font-medium text-text">{String(arg.event.title || "")}</span>
          <span className={`shrink-0 text-[11px] ${eventStatusClasses(status)}`}>{status}</span>
        </div>
        <div className="truncate text-[11px] text-text-muted">{scheduleLabel}</div>
      </div>
    );
  };

  return (
    <div className="grid gap-3">
      <div className="flex flex-wrap items-start justify-between gap-3">
        <div className="space-y-1">
          <div className="text-[10px] uppercase tracking-wide text-text-subtle">
            Scheduled automations
          </div>
          <div className="text-sm text-text-muted">
            Drag simple cron events to reschedule them. Interval schedules show up too, and you can
            click a day, +more link, or the selected hour to drill into crowded slots.
          </div>
        </div>
        <div className="flex flex-wrap items-center justify-end gap-2 text-xs">
          {currentView === "timeGridDay" && focusedSlotLabel ? (
            <span className="rounded-full border border-primary/20 bg-primary/10 px-3 py-1 text-text">
              {focusedSlotLabel}
            </span>
          ) : null}
          <span className="rounded-full border border-border bg-surface px-3 py-1 text-text-muted">
            {events.length} scheduled item{events.length === 1 ? "" : "s"}
          </span>
        </div>
      </div>
      <div className="desktop-automation-calendar rounded-xl border border-border bg-surface-elevated/30 p-3">
        <FullCalendar
          ref={calendarRef}
          plugins={[timeGridPlugin, interactionPlugin]}
          initialView="timeGridWeek"
          timeZone="UTC"
          firstDay={0}
          height="auto"
          expandRows
          nowIndicator
          editable
          eventStartEditable
          eventDurationEditable={false}
          eventOverlap
          slotEventOverlap
          allDaySlot={false}
          slotMinTime="00:00:00"
          slotMaxTime="24:00:00"
          scrollTimeReset={false}
          stickyHeaderDates
          navLinks
          headerToolbar={{
            left: "prev,next today",
            center: "title",
            right: "timeGridWeek,timeGridDay",
          }}
          buttonText={{
            timeGridWeek: "week",
            timeGridDay: "day",
            today: "today",
          }}
          views={{
            timeGridWeek: {
              eventMaxStack: 1,
            },
            timeGridDay: {
              slotDuration: "00:15:00",
              slotLabelInterval: "01:00:00",
            },
          }}
          events={events}
          datesSet={handleDatesSet}
          navLinkDayClick={(date) => {
            const fallbackTime = focusedSlot ? toScrollTime(focusedSlot) : DEFAULT_DAY_SCROLL_TIME;
            const { hour, minute } = timeParts(fallbackTime);
            focusDayAtTime(
              new Date(
                Date.UTC(
                  date.getUTCFullYear(),
                  date.getUTCMonth(),
                  date.getUTCDate(),
                  hour,
                  minute,
                  0,
                  0
                )
              )
            );
          }}
          dateClick={handleDateClick}
          moreLinkClick={handleMoreLinkClick}
          moreLinkClassNames={() => ["desktop-automation-calendar-more-link"]}
          moreLinkContent={(arg) => <span>+{arg.num} more</span>}
          eventClick={handleEventClick}
          eventDrop={onEventDrop}
          eventContent={renderEventContent}
          slotLaneClassNames={(arg) => {
            if (currentView !== "timeGridDay" || !focusedSlot || !arg.date) return [];
            return arg.date.getTime() === focusedSlot.getTime()
              ? ["desktop-automation-calendar-slot-focus"]
              : [];
          }}
          eventClassNames={() => ["desktop-automation-calendar-chip"]}
        />
      </div>
      <div className="rounded-xl border border-border bg-surface-elevated/20 p-3">
        <div className="flex flex-wrap items-start justify-between gap-3">
          <div className="space-y-1">
            <div className="text-[10px] uppercase tracking-wide text-text-subtle">
              Hour drill-down
            </div>
            <div className="text-xs text-text-muted">
              Click a slot or a +more link to inspect the automations in that UTC hour.
            </div>
          </div>
          <div className="flex flex-wrap items-center gap-2 text-xs">
            {focusedSlotLabel ? (
              <span className="rounded-full border border-primary/20 bg-primary/10 px-3 py-1 text-text">
                {focusedSlotLabel}
              </span>
            ) : null}
            {focusedSlot ? (
              <button
                type="button"
                className="rounded-full border border-border bg-surface px-3 py-1 text-text-muted transition hover:border-primary/40 hover:text-text"
                onClick={() => syncFocusedSlot(null)}
              >
                Clear
              </button>
            ) : null}
          </div>
        </div>
        {focusedSlot ? (
          <div className="mt-3 grid gap-2">
            <div className="text-sm font-medium text-text">
              {focusedHourLabel || "Selected hour"}
              {focusedHourEvents.length
                ? ` · ${focusedHourEvents.length} item${focusedHourEvents.length === 1 ? "" : "s"}`
                : ""}
            </div>
            {focusedHourEvents.length ? (
              <div className="grid max-h-80 gap-2 overflow-auto pr-1">
                {focusedHourEvents.map(({ event, start }) => {
                  const automation = event?.extendedProps?.automation as
                    | AutomationV2Spec
                    | undefined;
                  const automationId = String(
                    automation?.automation_id || event?.extendedProps?.automationId || ""
                  ).trim();
                  const title = String(
                    event?.title || automation?.name || automation?.automation_id || "Automation"
                  ).trim();
                  const scheduleLabel = String(event?.extendedProps?.scheduleLabel || "").trim();
                  const status =
                    String(event?.extendedProps?.status || "active").trim() || "active";
                  const family =
                    String(event?.extendedProps?.scheduleType || "").trim() || "schedule";
                  const timeLabel = start
                    ? new Intl.DateTimeFormat("en-US", {
                        hour: "numeric",
                        minute: "2-digit",
                        timeZone: "UTC",
                      }).format(start)
                    : "time unavailable";
                  return (
                    <button
                      key={String(event?.id || `${title}-${timeLabel}`)}
                      type="button"
                      className="grid gap-1 rounded-lg border border-border bg-surface/80 px-3 py-2 text-left transition hover:border-primary/40 hover:bg-primary/10"
                      onClick={() => automationId && void onOpenAutomation(automationId)}
                    >
                      <div className="flex flex-wrap items-center justify-between gap-2">
                        <div className="min-w-0">
                          <div className="truncate text-sm font-medium text-text">{title}</div>
                          <div className="text-xs text-text-muted">
                            {timeLabel} UTC · {family}
                          </div>
                        </div>
                        <div className="flex flex-wrap items-center gap-1 text-xs">
                          <span className="rounded-full border border-border bg-surface px-2 py-0.5 text-text-muted">
                            {status}
                          </span>
                        </div>
                      </div>
                      {scheduleLabel ? (
                        <div className="text-xs text-text-subtle">{scheduleLabel}</div>
                      ) : null}
                    </button>
                  );
                })}
              </div>
            ) : (
              <div className="rounded-lg border border-dashed border-border bg-surface px-3 py-2 text-xs text-text-subtle">
                No automations are scheduled in this hour.
              </div>
            )}
          </div>
        ) : (
          <div className="mt-3 rounded-lg border border-dashed border-border bg-surface px-3 py-2 text-xs text-text-subtle">
            Select any slot, or click +more on a crowded hour, to drill into the exact automations.
          </div>
        )}
      </div>
    </div>
  );
}
