import type { Category } from "../api/bindings";
import { NEUTRAL_THUMB, SegmentedControl } from "./SegmentedControl";

type Choice = "off" | Category;

const OPTIONS = [
  { value: "off", label: "Off" },
  { value: "productive", label: "Productive" },
  { value: "entertainment", label: "Entertainment" },
] as const satisfies readonly { value: Choice; label: string }[];

// The selected segment takes its category's color; Off stays neutral.
const thumb = (choice: Choice) =>
  choice === "productive"
    ? "border-productive/45 bg-productive/20"
    : choice === "entertainment"
      ? "border-entertainment/50 bg-entertainment/25"
      : NEUTRAL_THUMB;

/** Off · Productive · Entertainment. Off means not tracked. */
export function CategoryControl({
  name,
  value,
  onChange,
}: {
  /** What is being categorized, for the group's accessible name. */
  name: string;
  value: Category | null;
  onChange: (category: Category | null) => void;
}) {
  return (
    <SegmentedControl
      label={`Category for ${name}`}
      options={OPTIONS}
      value={value ?? "off"}
      onChange={(choice) => onChange(choice === "off" ? null : choice)}
      thumbClass={thumb}
      className="w-full"
    />
  );
}
