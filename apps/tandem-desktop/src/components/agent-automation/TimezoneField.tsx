import { useId } from "react";
import { Button, Input } from "@/components/ui";
import { COMMON_TIMEZONES, detectBrowserTimezone } from "./timezone";

type TimezoneFieldProps = {
  value: string;
  onChange: (value: string) => void;
  error?: string;
  label?: string;
  hint?: string;
  allowBrowserDefault?: boolean;
  className?: string;
};

export function TimezoneField({
  value,
  onChange,
  error,
  label = "Timezone",
  hint = "Use an IANA timezone like Europe/Berlin.",
  allowBrowserDefault = true,
  className = "",
}: TimezoneFieldProps) {
  const datalistId = useId();
  const browserTimezone = detectBrowserTimezone();

  return (
    <div className={className}>
      <div className="grid gap-2 md:grid-cols-[minmax(0,1fr)_auto] md:items-end">
        <Input
          label={label}
          value={value}
          error={error}
          placeholder={browserTimezone}
          onChange={(event) => onChange(event.target.value)}
          list={datalistId}
          className="font-mono"
        />
        {allowBrowserDefault ? (
          <Button
            type="button"
            variant="secondary"
            size="md"
            className="md:mb-[1.85rem]"
            onClick={() => onChange(browserTimezone)}
          >
            Use browser
          </Button>
        ) : null}
      </div>
      <div className="mt-1 text-sm text-text-muted">{hint}</div>
      <datalist id={datalistId}>
        {COMMON_TIMEZONES.map((timezone) => (
          <option key={timezone} value={timezone} />
        ))}
      </datalist>
    </div>
  );
}
