import * as CheckboxPrimitive from "@radix-ui/react-checkbox";
import * as LabelPrimitive from "@radix-ui/react-label";
import * as PopoverPrimitive from "@radix-ui/react-popover";
import * as SelectPrimitive from "@radix-ui/react-select";
import * as SliderPrimitive from "@radix-ui/react-slider";
import { Slot } from "@radix-ui/react-slot";
import * as SwitchPrimitive from "@radix-ui/react-switch";
import * as ToggleGroupPrimitive from "@radix-ui/react-toggle-group";
import clsx from "clsx";
import { Check, ChevronDown, Plus, Trash2, X } from "../../icons";
import {
  forwardRef,
  useId,
  useMemo,
  useState,
  type ButtonHTMLAttributes,
  type InputHTMLAttributes,
  type KeyboardEvent,
  type ReactNode,
  type TextareaHTMLAttributes,
} from "react";
import styles from "./Forms.module.css";

export interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: "primary" | "secondary" | "ghost" | "danger";
  size?: "sm" | "md" | "lg";
  asChild?: boolean;
}

export const Button = forwardRef<HTMLButtonElement, ButtonProps>(function Button(
  { variant = "secondary", size = "md", asChild = false, className, type = "button", ...props },
  ref,
) {
  const Comp = asChild ? Slot : "button";
  return (
    <Comp
      ref={ref}
      type={asChild ? undefined : type}
      className={clsx(
        styles.button,
        size === "sm" && styles.btnSm,
        size === "md" && styles.btnMd,
        size === "lg" && styles.btnLg,
        variant === "primary" && styles.btnPrimary,
        variant === "secondary" && styles.btnSecondary,
        variant === "ghost" && styles.btnGhost,
        variant === "danger" && styles.btnDanger,
        className,
      )}
      {...props}
    />
  );
});

export interface IconButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  label: string;
  variant?: "primary" | "secondary" | "ghost" | "danger";
  size?: "sm" | "md";
}

export const IconButton = forwardRef<HTMLButtonElement, IconButtonProps>(function IconButton(
  { label, variant = "ghost", size = "md", className, type = "button", children, ...props },
  ref,
) {
  return (
    <button
      ref={ref}
      type={type}
      aria-label={label}
      title={label}
      className={clsx(
        styles.button,
        styles.iconButton,
        size === "sm" && styles.iconButtonSm,
        variant === "primary" && styles.btnPrimary,
        variant === "secondary" && styles.btnSecondary,
        variant === "ghost" && styles.btnGhost,
        variant === "danger" && styles.btnDanger,
        className,
      )}
      {...props}
    >
      {children}
    </button>
  );
});

export interface LabelProps {
  htmlFor?: string;
  required?: boolean;
  children: ReactNode;
  className?: string;
}

export function Label({ htmlFor, required, children, className }: LabelProps) {
  return (
    <LabelPrimitive.Root htmlFor={htmlFor} className={clsx(styles.label, className)}>
      {children}
      {required && (
        <span className={styles.requiredMark} aria-hidden="true">
          *
        </span>
      )}
    </LabelPrimitive.Root>
  );
}

export interface FormFieldProps {
  label: string;
  htmlFor?: string;
  required?: boolean;
  hint?: string;
  error?: string;
  children: ReactNode;
  className?: string;
}

export function FormField({
  label,
  htmlFor,
  required,
  hint,
  error,
  children,
  className,
}: FormFieldProps) {
  return (
    <div className={clsx(styles.formField, className)}>
      <Label htmlFor={htmlFor} required={required}>
        {label}
      </Label>
      {children}
      {hint && !error && <span className={styles.fieldHint}>{hint}</span>}
      {error && (
        <span role="alert" className={styles.fieldError}>
          {error}
        </span>
      )}
    </div>
  );
}

export interface InputProps extends InputHTMLAttributes<HTMLInputElement> {
  invalid?: boolean;
  mono?: boolean;
}

export const Input = forwardRef<HTMLInputElement, InputProps>(function Input(
  { invalid, mono, className, ...props },
  ref,
) {
  return (
    <input
      ref={ref}
      aria-invalid={invalid || undefined}
      className={clsx(
        styles.input,
        invalid && styles.inputInvalid,
        mono && styles.inputMono,
        className,
      )}
      {...props}
    />
  );
});

export interface TextareaProps extends TextareaHTMLAttributes<HTMLTextAreaElement> {
  invalid?: boolean;
  mono?: boolean;
}

export const Textarea = forwardRef<HTMLTextAreaElement, TextareaProps>(function Textarea(
  { invalid, mono, className, ...props },
  ref,
) {
  return (
    <textarea
      ref={ref}
      aria-invalid={invalid || undefined}
      className={clsx(
        styles.textarea,
        invalid && styles.inputInvalid,
        mono && styles.inputMono,
        className,
      )}
      {...props}
    />
  );
});

export interface SelectOption {
  value: string;
  label: string;
  disabled?: boolean;
  /** Optional group heading; consecutive options with the same group render under one label. */
  group?: string;
}

export interface SelectProps {
  value?: string;
  onValueChange: (value: string) => void;
  options: SelectOption[];
  placeholder?: string;
  disabled?: boolean;
  ariaLabel?: string;
  id?: string;
  className?: string;
}

function groupSelectOptions(options: SelectOption[]) {
  const sections: { group?: string; options: SelectOption[] }[] = [];
  for (const opt of options) {
    const last = sections[sections.length - 1];
    if (last && last.group === opt.group) last.options.push(opt);
    else sections.push({ group: opt.group, options: [opt] });
  }
  return sections;
}

function renderSelectItem(opt: SelectOption) {
  return (
    <SelectPrimitive.Item
      key={opt.value}
      value={opt.value}
      disabled={opt.disabled}
      className={styles.selectItem}
    >
      <SelectPrimitive.ItemText>{opt.label}</SelectPrimitive.ItemText>
      <SelectPrimitive.ItemIndicator>
        <Check size={14} aria-hidden="true" />
      </SelectPrimitive.ItemIndicator>
    </SelectPrimitive.Item>
  );
}

export function Select({
  value,
  onValueChange,
  options,
  placeholder = "Select...",
  disabled,
  ariaLabel,
  id,
  className,
}: SelectProps) {
  return (
    <SelectPrimitive.Root value={value} onValueChange={onValueChange} disabled={disabled}>
      <SelectPrimitive.Trigger
        id={id}
        aria-label={ariaLabel}
        className={clsx(styles.selectTrigger, className)}
      >
        <SelectPrimitive.Value placeholder={placeholder} />
        <SelectPrimitive.Icon>
          <ChevronDown size={14} aria-hidden="true" />
        </SelectPrimitive.Icon>
      </SelectPrimitive.Trigger>
      <SelectPrimitive.Portal>
        <SelectPrimitive.Content position="popper" sideOffset={4} className={styles.selectContent}>
          <SelectPrimitive.Viewport>
            {groupSelectOptions(options).map((section, idx) =>
              section.group ? (
                <SelectPrimitive.Group key={`${section.group}-${idx}`}>
                  {idx > 0 && <SelectPrimitive.Separator className={styles.selectSeparator} />}
                  <SelectPrimitive.Label className={styles.selectGroupLabel}>
                    {section.group}
                  </SelectPrimitive.Label>
                  {section.options.map(renderSelectItem)}
                </SelectPrimitive.Group>
              ) : (
                section.options.map(renderSelectItem)
              ),
            )}
          </SelectPrimitive.Viewport>
        </SelectPrimitive.Content>
      </SelectPrimitive.Portal>
    </SelectPrimitive.Root>
  );
}

export interface ComboboxProps {
  options: SelectOption[];
  value: string;
  onChange: (value: string) => void;
  placeholder?: string;
  ariaLabel?: string;
  disabled?: boolean;
}

export function Combobox({
  options,
  value,
  onChange,
  placeholder = "Search or choose...",
  ariaLabel,
  disabled,
}: ComboboxProps) {
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");
  const selectedLabel = options.find((o) => o.value === value)?.label ?? value;
  const filtered = options.filter(
    (o) =>
      o.label.toLowerCase().includes(query.toLowerCase()) ||
      o.value.toLowerCase().includes(query.toLowerCase()),
  );

  return (
    <div className={styles.comboboxWrap}>
      <Input
        aria-label={ariaLabel}
        role="combobox"
        aria-expanded={open}
        aria-autocomplete="list"
        disabled={disabled}
        placeholder={placeholder}
        value={open ? query : selectedLabel}
        onFocus={() => {
          setQuery("");
          setOpen(true);
        }}
        onBlur={() => window.setTimeout(() => setOpen(false), 120)}
        onChange={(e) => {
          setQuery(e.target.value);
          setOpen(true);
        }}
      />
      {open && filtered.length > 0 && (
        <div role="listbox" className={styles.comboboxDropdown}>
          {filtered.map((opt) => (
            <button
              key={opt.value}
              type="button"
              role="option"
              aria-selected={opt.value === value}
              className={styles.comboboxOption}
              onMouseDown={(e) => {
                e.preventDefault();
                onChange(opt.value);
                setQuery("");
                setOpen(false);
              }}
            >
              <span>{opt.label}</span>
              {opt.value === value && <Check size={14} aria-hidden="true" />}
            </button>
          ))}
        </div>
      )}
    </div>
  );
}

export interface SuggestOption {
  /** The value written into the field when the option is picked. */
  value: string;
  /** Optional display label; defaults to `value`. */
  label?: string;
  /** Short muted hint shown on the right, e.g. "built-in" or "seen 2h ago". */
  description?: string;
  /** Options sharing a group are listed under one heading, in first-seen order. */
  group?: string;
}

function filterSuggestions(options: SuggestOption[], query: string, exclude?: string[]) {
  const q = query.trim().toLowerCase();
  const seen = new Set<string>();
  const out: SuggestOption[] = [];
  for (const opt of options) {
    if (seen.has(opt.value) || exclude?.includes(opt.value)) continue;
    seen.add(opt.value);
    if (
      !q ||
      opt.value.toLowerCase().includes(q) ||
      (opt.label?.toLowerCase().includes(q) ?? false)
    ) {
      out.push(opt);
    }
  }
  return out;
}

interface SuggestListProps {
  id: string;
  options: SuggestOption[];
  activeIndex: number;
  selectedValue?: string;
  onPick: (value: string) => void;
  onHover: (index: number) => void;
}

function SuggestList({ id, options, activeIndex, selectedValue, onPick, onHover }: SuggestListProps) {
  let lastGroup: string | undefined;
  return (
    <div id={id} role="listbox" className={styles.suggestList}>
      {options.map((opt, i) => {
        const header =
          opt.group && opt.group !== lastGroup ? (
            <div key={`g-${opt.group}`} role="presentation" className={styles.suggestGroup}>
              {opt.group}
            </div>
          ) : null;
        lastGroup = opt.group;
        return [
          header,
          <div
            key={opt.value}
            id={`${id}-opt-${i}`}
            role="option"
            aria-selected={i === activeIndex}
            data-active={i === activeIndex || undefined}
            className={styles.suggestOption}
            onMouseDown={(e) => {
              e.preventDefault();
              onPick(opt.value);
            }}
            onMouseMove={() => onHover(i)}
          >
            <span className={styles.suggestValue}>{opt.label ?? opt.value}</span>
            <span className={styles.suggestMeta}>
              {opt.description && <span>{opt.description}</span>}
              {opt.value === selectedValue && <Check size={14} aria-hidden="true" />}
            </span>
          </div>,
        ];
      })}
    </div>
  );
}

interface SuggestControllerArgs {
  options: SuggestOption[];
  query: string;
  exclude?: string[];
  onPick: (value: string) => void;
}

function useSuggestController({ options, query, exclude, onPick }: SuggestControllerArgs) {
  const [open, setOpen] = useState(false);
  const [activeIndex, setActiveIndex] = useState(-1);
  const filtered = useMemo(
    () => filterSuggestions(options, query, exclude),
    [options, query, exclude],
  );
  const visible = open && filtered.length > 0;

  const pick = (value: string) => {
    onPick(value);
    setActiveIndex(-1);
  };

  /** Returns true when the key was consumed by the suggestion list. */
  const handleKey = (e: KeyboardEvent<HTMLInputElement>): boolean => {
    if (e.key === "ArrowDown") {
      e.preventDefault();
      setOpen(true);
      setActiveIndex((i) => (filtered.length === 0 ? -1 : (i + 1) % filtered.length));
      return true;
    }
    if (e.key === "ArrowUp") {
      e.preventDefault();
      setOpen(true);
      setActiveIndex((i) =>
        filtered.length === 0 ? -1 : (i - 1 + filtered.length) % filtered.length,
      );
      return true;
    }
    if (e.key === "Enter" && visible && activeIndex >= 0 && filtered[activeIndex]) {
      e.preventDefault();
      pick(filtered[activeIndex].value);
      return true;
    }
    if (e.key === "Escape" && visible) {
      e.preventDefault();
      setOpen(false);
      setActiveIndex(-1);
      return true;
    }
    return false;
  };

  return { open, setOpen, activeIndex, setActiveIndex, filtered, visible, pick, handleKey };
}

export interface SuggestInputProps {
  value: string;
  onChange: (value: string) => void;
  /** Preset and discovered values offered in the dropdown. Free text is always allowed. */
  suggestions: SuggestOption[];
  placeholder?: string;
  ariaLabel?: string;
  disabled?: boolean;
  mono?: boolean;
  id?: string;
  className?: string;
}

/**
 * Text input with a searchable suggestion dropdown. Unlike `Combobox`, any typed
 * value is kept, so presets speed up common choices without blocking custom ones.
 */
export function SuggestInput({
  value,
  onChange,
  suggestions,
  placeholder,
  ariaLabel,
  disabled,
  mono,
  id,
  className,
}: SuggestInputProps) {
  const listId = useId();
  // Show every suggestion on focus; filter only once the user starts typing.
  const [query, setQuery] = useState("");
  const ctl = useSuggestController({
    options: suggestions,
    query,
    onPick: (v) => {
      onChange(v);
      setQuery("");
      ctl.setOpen(false);
    },
  });

  return (
    <PopoverPrimitive.Root open={ctl.visible && !disabled}>
      <PopoverPrimitive.Anchor asChild>
        <div className={clsx(styles.suggestWrap, className)}>
          <Input
            id={id}
            role="combobox"
            aria-label={ariaLabel}
            aria-expanded={ctl.visible}
            aria-controls={listId}
            aria-autocomplete="list"
            aria-activedescendant={
              ctl.visible && ctl.activeIndex >= 0 ? `${listId}-opt-${ctl.activeIndex}` : undefined
            }
            autoComplete="off"
            spellCheck={false}
            value={value}
            placeholder={placeholder}
            disabled={disabled}
            mono={mono}
            className={styles.suggestInput}
            onFocus={() => {
              setQuery("");
              ctl.setActiveIndex(-1);
              ctl.setOpen(true);
            }}
            onBlur={() => {
              ctl.setOpen(false);
              ctl.setActiveIndex(-1);
            }}
            onChange={(e) => {
              onChange(e.target.value);
              setQuery(e.target.value);
              ctl.setActiveIndex(-1);
              ctl.setOpen(true);
            }}
            onKeyDown={(e) => {
              ctl.handleKey(e);
            }}
          />
          <ChevronDown size={14} aria-hidden="true" className={styles.suggestChevron} />
        </div>
      </PopoverPrimitive.Anchor>
      <PopoverPrimitive.Portal>
        <PopoverPrimitive.Content
          side="bottom"
          align="start"
          sideOffset={4}
          className={styles.suggestContent}
          onOpenAutoFocus={(e) => e.preventDefault()}
          onCloseAutoFocus={(e) => e.preventDefault()}
        >
          <SuggestList
            id={listId}
            options={ctl.filtered}
            activeIndex={ctl.activeIndex}
            selectedValue={value}
            onPick={ctl.pick}
            onHover={ctl.setActiveIndex}
          />
        </PopoverPrimitive.Content>
      </PopoverPrimitive.Portal>
    </PopoverPrimitive.Root>
  );
}

export interface CheckboxProps {
  checked: boolean;
  onCheckedChange: (checked: boolean) => void;
  label?: string;
  disabled?: boolean;
  id?: string;
}

export function Checkbox({ checked, onCheckedChange, label, disabled, id }: CheckboxProps) {
  const generatedId = useId();
  const controlId = id ?? generatedId;
  return (
    <div className={styles.checkboxRow}>
      <CheckboxPrimitive.Root
        id={controlId}
        checked={checked}
        disabled={disabled}
        onCheckedChange={(state) => onCheckedChange(state === true)}
        className={styles.checkbox}
      >
        <CheckboxPrimitive.Indicator>
          <Check size={12} aria-hidden="true" />
        </CheckboxPrimitive.Indicator>
      </CheckboxPrimitive.Root>
      {label && <Label htmlFor={controlId}>{label}</Label>}
    </div>
  );
}

export interface SwitchProps {
  checked: boolean;
  onCheckedChange: (checked: boolean) => void;
  label?: string;
  ariaLabel?: string;
  disabled?: boolean;
  id?: string;
  "data-testid"?: string;
}

export function Switch({
  checked,
  onCheckedChange,
  label,
  ariaLabel,
  disabled,
  id,
  "data-testid": testId,
}: SwitchProps) {
  const generatedId = useId();
  const controlId = id ?? generatedId;
  return (
    <div className={styles.switchRow}>
      <SwitchPrimitive.Root
        id={controlId}
        checked={checked}
        disabled={disabled}
        aria-label={ariaLabel ?? label}
        data-testid={testId}
        onCheckedChange={onCheckedChange}
        className={styles.switchRoot}
      >
        <SwitchPrimitive.Thumb className={styles.switchThumb} />
      </SwitchPrimitive.Root>
      {label && <Label htmlFor={controlId}>{label}</Label>}
    </div>
  );
}

export interface ToggleGroupItemOption {
  value: string;
  label: string;
}

export interface ToggleGroupProps {
  value: string;
  onValueChange: (value: string) => void;
  options: ToggleGroupItemOption[];
  ariaLabel?: string;
}

export function ToggleGroup({ value, onValueChange, options, ariaLabel }: ToggleGroupProps) {
  return (
    <ToggleGroupPrimitive.Root
      type="single"
      value={value}
      aria-label={ariaLabel}
      onValueChange={(next) => {
        if (next) onValueChange(next);
      }}
      className={styles.toggleGroup}
    >
      {options.map((opt) => (
        <ToggleGroupPrimitive.Item
          key={opt.value}
          value={opt.value}
          className={styles.toggleGroupItem}
        >
          {opt.label}
        </ToggleGroupPrimitive.Item>
      ))}
    </ToggleGroupPrimitive.Root>
  );
}

export interface SliderProps {
  value: number;
  min?: number;
  max?: number;
  step?: number;
  onValueChange: (value: number) => void;
  ariaLabel?: string;
  disabled?: boolean;
}

export function Slider({
  value,
  min = 0,
  max = 100,
  step = 1,
  onValueChange,
  ariaLabel = "Slider",
  disabled,
}: SliderProps) {
  return (
    <SliderPrimitive.Root
      value={[value]}
      min={min}
      max={max}
      step={step}
      disabled={disabled}
      onValueChange={(vals) => {
        if (typeof vals[0] === "number") onValueChange(vals[0]);
      }}
      className={styles.sliderRoot}
    >
      <SliderPrimitive.Track className={styles.sliderTrack}>
        <SliderPrimitive.Range className={styles.sliderRange} />
      </SliderPrimitive.Track>
      <SliderPrimitive.Thumb aria-label={ariaLabel} className={styles.sliderThumb} />
    </SliderPrimitive.Root>
  );
}

export interface TagInputProps {
  values: string[];
  onChange: (values: string[]) => void;
  placeholder?: string;
  ariaLabel?: string;
  disabled?: boolean;
  /** Optional values offered in a dropdown while typing. Free text is still allowed. */
  suggestions?: SuggestOption[];
}

export function TagInput({
  values,
  onChange,
  placeholder = "Type and press Enter...",
  ariaLabel = "Tag input",
  disabled,
  suggestions,
}: TagInputProps) {
  const [draft, setDraft] = useState("");
  const listId = useId();
  const hasSuggestions = Boolean(suggestions && suggestions.length > 0);

  const addValue = (raw: string) => {
    const trimmed = raw.trim();
    if (!trimmed) return;
    if (!values.includes(trimmed)) {
      onChange([...values, trimmed]);
    }
    setDraft("");
  };

  const ctl = useSuggestController({
    options: suggestions ?? [],
    query: draft,
    exclude: values,
    onPick: addValue,
  });

  const handleKeyDown = (e: KeyboardEvent<HTMLInputElement>) => {
    if (hasSuggestions && ctl.handleKey(e)) return;
    if (e.key === "Enter" || e.key === ",") {
      e.preventDefault();
      addValue(draft);
    } else if (e.key === "Backspace" && !draft && values.length > 0) {
      onChange(values.slice(0, -1));
    }
  };

  const box = (
    <div className={styles.tagBox}>
      {values.map((val) => (
        <span key={val} className={styles.tagChip}>
          {val}
          {!disabled && (
            <button
              type="button"
              aria-label={`Remove ${val}`}
              className={styles.tagRemoveBtn}
              onClick={() => onChange(values.filter((v) => v !== val))}
            >
              <X size={12} aria-hidden="true" />
            </button>
          )}
        </span>
      ))}
      <input
        type="text"
        aria-label={ariaLabel}
        disabled={disabled}
        value={draft}
        placeholder={values.length === 0 ? placeholder : ""}
        autoComplete="off"
        {...(hasSuggestions
          ? {
              role: "combobox",
              "aria-expanded": ctl.visible,
              "aria-controls": listId,
              "aria-autocomplete": "list" as const,
              "aria-activedescendant":
                ctl.visible && ctl.activeIndex >= 0
                  ? `${listId}-opt-${ctl.activeIndex}`
                  : undefined,
            }
          : {})}
        onFocus={() => hasSuggestions && ctl.setOpen(true)}
        onChange={(e) => {
          setDraft(e.target.value);
          ctl.setActiveIndex(-1);
          if (hasSuggestions) ctl.setOpen(true);
        }}
        onKeyDown={handleKeyDown}
        onBlur={() => {
          ctl.setOpen(false);
          addValue(draft);
        }}
        className={styles.tagInputField}
      />
    </div>
  );

  if (!hasSuggestions) return box;

  return (
    <PopoverPrimitive.Root open={ctl.visible && !disabled}>
      <PopoverPrimitive.Anchor asChild>{box}</PopoverPrimitive.Anchor>
      <PopoverPrimitive.Portal>
        <PopoverPrimitive.Content
          side="bottom"
          align="start"
          sideOffset={4}
          className={styles.suggestContent}
          onOpenAutoFocus={(e) => e.preventDefault()}
          onCloseAutoFocus={(e) => e.preventDefault()}
        >
          <SuggestList
            id={listId}
            options={ctl.filtered}
            activeIndex={ctl.activeIndex}
            onPick={ctl.pick}
            onHover={ctl.setActiveIndex}
          />
        </PopoverPrimitive.Content>
      </PopoverPrimitive.Portal>
    </PopoverPrimitive.Root>
  );
}

export interface KeyValuePair {
  key: string;
  value: string;
}

export interface KeyValueEditorProps {
  pairs: KeyValuePair[];
  onChange: (pairs: KeyValuePair[]) => void;
  keyPlaceholder?: string;
  valuePlaceholder?: string;
  disabled?: boolean;
}

export function KeyValueEditor({
  pairs,
  onChange,
  keyPlaceholder = "Attribute key",
  valuePlaceholder = "Value",
  disabled,
}: KeyValueEditorProps) {
  const update = (idx: number, field: "key" | "value", next: string) => {
    const copy = pairs.map((p, i) => (i === idx ? { ...p, [field]: next } : p));
    onChange(copy);
  };

  return (
    <div className={styles.kvList}>
      {pairs.map((pair, idx) => (
        <div key={idx} className={styles.kvRow}>
          <Input
            aria-label={`Key ${idx + 1}`}
            placeholder={keyPlaceholder}
            value={pair.key}
            disabled={disabled}
            mono
            onChange={(e) => update(idx, "key", e.target.value)}
          />
          <Input
            aria-label={`Value ${idx + 1}`}
            placeholder={valuePlaceholder}
            value={pair.value}
            disabled={disabled}
            mono
            onChange={(e) => update(idx, "value", e.target.value)}
          />
          <IconButton
            label={`Remove row ${idx + 1}`}
            size="sm"
            disabled={disabled}
            onClick={() => onChange(pairs.filter((_, i) => i !== idx))}
          >
            <Trash2 size={14} aria-hidden="true" />
          </IconButton>
        </div>
      ))}
      {!disabled && (
        <div>
          <Button
            size="sm"
            variant="ghost"
            onClick={() => onChange([...pairs, { key: "", value: "" }])}
          >
            <Plus size={14} aria-hidden="true" />
            Add attribute
          </Button>
        </div>
      )}
    </div>
  );
}
