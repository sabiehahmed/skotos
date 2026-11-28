import * as AlertDialogPrimitive from "@radix-ui/react-alert-dialog";
import * as DialogPrimitive from "@radix-ui/react-dialog";
import * as DropdownMenuPrimitive from "@radix-ui/react-dropdown-menu";
import * as PopoverPrimitive from "@radix-ui/react-popover";
import * as TabsPrimitive from "@radix-ui/react-tabs";
import * as ToastPrimitive from "@radix-ui/react-toast";
import * as TooltipPrimitive from "@radix-ui/react-tooltip";
import clsx from "clsx";
import { ChevronRight, Search, X } from "../../icons";
import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useState,
  type ReactNode,
} from "react";
import { Button, IconButton } from "../forms/Forms";
import styles from "./Overlays.module.css";

// =============================================================================
// Dialog & Drawer
// =============================================================================

export interface DialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  title: string;
  description?: string;
  wide?: boolean;
  footer?: ReactNode;
  children: ReactNode;
}

export function Dialog({
  open,
  onOpenChange,
  title,
  description,
  wide = false,
  footer,
  children,
}: DialogProps) {
  return (
    <DialogPrimitive.Root open={open} onOpenChange={onOpenChange}>
      <DialogPrimitive.Portal>
        <DialogPrimitive.Overlay className={styles.overlay} />
        <DialogPrimitive.Content className={clsx(styles.dialogContent, wide && styles.dialogWide)}>
          <div className={styles.dialogHeader}>
            <div>
              <DialogPrimitive.Title className={styles.dialogTitle}>{title}</DialogPrimitive.Title>
              {description && (
                <DialogPrimitive.Description className={styles.dialogDescription}>
                  {description}
                </DialogPrimitive.Description>
              )}
            </div>
            <DialogPrimitive.Close asChild>
              <IconButton label="Close dialog" size="sm">
                <X size={16} aria-hidden="true" />
              </IconButton>
            </DialogPrimitive.Close>
          </div>
          <div>{children}</div>
          {footer && <div className={styles.dialogFooter}>{footer}</div>}
        </DialogPrimitive.Content>
      </DialogPrimitive.Portal>
    </DialogPrimitive.Root>
  );
}

export interface AlertDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  title: string;
  description: string;
  confirmLabel?: string;
  cancelLabel?: string;
  confirmDisabled?: boolean;
  destructive?: boolean;
  onConfirm: () => void;
  children?: ReactNode;
}

export function AlertDialog({
  open,
  onOpenChange,
  title,
  description,
  confirmLabel = "Confirm",
  cancelLabel = "Cancel",
  confirmDisabled = false,
  destructive = true,
  onConfirm,
  children,
}: AlertDialogProps) {
  return (
    <AlertDialogPrimitive.Root open={open} onOpenChange={onOpenChange}>
      <AlertDialogPrimitive.Portal>
        <AlertDialogPrimitive.Overlay className={styles.overlay} />
        <AlertDialogPrimitive.Content className={styles.dialogContent}>
          <div>
            <AlertDialogPrimitive.Title className={styles.dialogTitle}>
              {title}
            </AlertDialogPrimitive.Title>
            <AlertDialogPrimitive.Description className={styles.dialogDescription}>
              {description}
            </AlertDialogPrimitive.Description>
          </div>
          {children && <div>{children}</div>}
          <div className={styles.dialogFooter}>
            <AlertDialogPrimitive.Cancel asChild>
              <Button variant="secondary">{cancelLabel}</Button>
            </AlertDialogPrimitive.Cancel>
            <AlertDialogPrimitive.Action asChild>
              <Button
                variant={destructive ? "danger" : "primary"}
                disabled={confirmDisabled}
                onClick={onConfirm}
              >
                {confirmLabel}
              </Button>
            </AlertDialogPrimitive.Action>
          </div>
        </AlertDialogPrimitive.Content>
      </AlertDialogPrimitive.Portal>
    </AlertDialogPrimitive.Root>
  );
}

export interface DrawerProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  title: string;
  description?: string;
  footer?: ReactNode;
  children: ReactNode;
}

export function Drawer({
  open,
  onOpenChange,
  title,
  description,
  footer,
  children,
}: DrawerProps) {
  return (
    <DialogPrimitive.Root open={open} onOpenChange={onOpenChange}>
      <DialogPrimitive.Portal>
        <DialogPrimitive.Overlay className={styles.overlay} />
        <DialogPrimitive.Content className={styles.drawerContent}>
          <div className={styles.dialogHeader}>
            <div>
              <DialogPrimitive.Title className={styles.dialogTitle}>{title}</DialogPrimitive.Title>
              {description && (
                <DialogPrimitive.Description className={styles.dialogDescription}>
                  {description}
                </DialogPrimitive.Description>
              )}
            </div>
            <DialogPrimitive.Close asChild>
              <IconButton label="Close drawer" size="sm">
                <X size={16} aria-hidden="true" />
              </IconButton>
            </DialogPrimitive.Close>
          </div>
          <div style={{ flex: 1 }}>{children}</div>
          {footer && <div className={styles.dialogFooter}>{footer}</div>}
        </DialogPrimitive.Content>
      </DialogPrimitive.Portal>
    </DialogPrimitive.Root>
  );
}

// =============================================================================
// Popover, DropdownMenu & Tooltip
// =============================================================================

export interface PopoverProps {
  trigger: ReactNode;
  children: ReactNode;
  open?: boolean;
  onOpenChange?: (open: boolean) => void;
}

export function Popover({ trigger, children, open, onOpenChange }: PopoverProps) {
  return (
    <PopoverPrimitive.Root open={open} onOpenChange={onOpenChange}>
      <PopoverPrimitive.Trigger asChild>{trigger}</PopoverPrimitive.Trigger>
      <PopoverPrimitive.Portal>
        <PopoverPrimitive.Content sideOffset={6} className={styles.popoverContent}>
          {children}
        </PopoverPrimitive.Content>
      </PopoverPrimitive.Portal>
    </PopoverPrimitive.Root>
  );
}

export interface DropdownMenuItem {
  label: string;
  icon?: ReactNode;
  danger?: boolean;
  disabled?: boolean;
  onSelect: () => void;
}

export interface DropdownMenuProps {
  trigger: ReactNode;
  items: DropdownMenuItem[];
}

export function DropdownMenu({ trigger, items }: DropdownMenuProps) {
  return (
    <DropdownMenuPrimitive.Root>
      <DropdownMenuPrimitive.Trigger asChild>{trigger}</DropdownMenuPrimitive.Trigger>
      <DropdownMenuPrimitive.Portal>
        <DropdownMenuPrimitive.Content sideOffset={6} align="end" className={styles.menuContent}>
          {items.map((item) => (
            <DropdownMenuPrimitive.Item
              key={item.label}
              disabled={item.disabled}
              onSelect={item.onSelect}
              className={clsx(styles.menuItem, item.danger && styles.menuItemDanger)}
            >
              {item.icon}
              <span>{item.label}</span>
            </DropdownMenuPrimitive.Item>
          ))}
        </DropdownMenuPrimitive.Content>
      </DropdownMenuPrimitive.Portal>
    </DropdownMenuPrimitive.Root>
  );
}

export const TooltipProvider = TooltipPrimitive.Provider;

export interface TooltipProps {
  content: ReactNode;
  children: ReactNode;
}

export function Tooltip({ content, children }: TooltipProps) {
  return (
    <TooltipPrimitive.Root delayDuration={200}>
      <TooltipPrimitive.Trigger asChild>{children}</TooltipPrimitive.Trigger>
      <TooltipPrimitive.Portal>
        <TooltipPrimitive.Content sideOffset={6} className={styles.tooltipContent}>
          {content}
        </TooltipPrimitive.Content>
      </TooltipPrimitive.Portal>
    </TooltipPrimitive.Root>
  );
}

// =============================================================================
// Tabs, Breadcrumb, Pagination & CommandPalette
// =============================================================================

export interface TabItem {
  value: string;
  label: ReactNode;
  content: ReactNode;
}

export interface TabsProps {
  value?: string;
  defaultValue?: string;
  onValueChange?: (value: string) => void;
  items: TabItem[];
}

export function Tabs({ value, defaultValue, onValueChange, items }: TabsProps) {
  return (
    <TabsPrimitive.Root
      value={value}
      defaultValue={defaultValue ?? items[0]?.value}
      onValueChange={onValueChange}
    >
      <TabsPrimitive.List className={styles.tabsList}>
        {items.map((tab) => (
          <TabsPrimitive.Trigger
            key={tab.value}
            value={tab.value}
            className={styles.tabsTrigger}
          >
            {tab.label}
          </TabsPrimitive.Trigger>
        ))}
      </TabsPrimitive.List>
      {items.map((tab) => (
        <TabsPrimitive.Content key={tab.value} value={tab.value}>
          {tab.content}
        </TabsPrimitive.Content>
      ))}
    </TabsPrimitive.Root>
  );
}

export interface BreadcrumbItem {
  label: string;
  onClick?: () => void;
}

export interface BreadcrumbProps {
  items: BreadcrumbItem[];
}

export function Breadcrumb({ items }: BreadcrumbProps) {
  return (
    <nav aria-label="Breadcrumb">
      <ol className={styles.breadcrumbList}>
        {items.map((item, idx) => (
          <li key={item.label} style={{ display: "inline-flex", alignItems: "center", gap: 6 }}>
            {idx > 0 && <ChevronRight size={12} className={styles.breadcrumbSep} aria-hidden="true" />}
            {item.onClick && idx < items.length - 1 ? (
              <Button size="sm" variant="ghost" onClick={item.onClick}>
                {item.label}
              </Button>
            ) : (
              <span aria-current={idx === items.length - 1 ? "page" : undefined}>{item.label}</span>
            )}
          </li>
        ))}
      </ol>
    </nav>
  );
}

export interface PaginationProps {
  hasNext: boolean;
  hasPrev?: boolean;
  summary?: string;
  onNext: () => void;
  onPrev?: () => void;
}

export function Pagination({
  hasNext,
  hasPrev = false,
  summary,
  onNext,
  onPrev,
}: PaginationProps) {
  return (
    <div className={styles.pagination}>
      <span style={{ fontSize: "var(--sk-text-xs)", color: "var(--sk-color-text-muted)" }}>
        {summary ?? ""}
      </span>
      <div style={{ display: "flex", gap: "var(--sk-space-2)" }}>
        {onPrev && (
          <Button size="sm" variant="secondary" disabled={!hasPrev} onClick={onPrev}>
            Previous
          </Button>
        )}
        <Button size="sm" variant="secondary" disabled={!hasNext} onClick={onNext}>
          Next
        </Button>
      </div>
    </div>
  );
}

export interface CommandItem {
  id: string;
  label: string;
  hint?: string;
  onSelect: () => void;
}

export interface CommandPaletteProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  items: CommandItem[];
  placeholder?: string;
}

export function CommandPalette({
  open,
  onOpenChange,
  items,
  placeholder = "Jump to flags, environments, settings...",
}: CommandPaletteProps) {
  const [query, setQuery] = useState("");

  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "k") {
        e.preventDefault();
        onOpenChange(!open);
      }
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [open, onOpenChange]);

  const filtered = items.filter((item) =>
    item.label.toLowerCase().includes(query.toLowerCase()),
  );

  return (
    <Dialog
      open={open}
      onOpenChange={(next) => {
        if (!next) setQuery("");
        onOpenChange(next);
      }}
      title="Command Palette"
    >
      <div className={styles.commandInputWrap}>
        <Search size={16} aria-hidden="true" />
        <input
          type="text"
          autoFocus
          aria-label="Search commands"
          placeholder={placeholder}
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          className={styles.commandInput}
        />
      </div>
      <div className={styles.commandList}>
        {filtered.map((item) => (
          <button
            key={item.id}
            type="button"
            className={styles.commandItem}
            onClick={() => {
              onOpenChange(false);
              setQuery("");
              item.onSelect();
            }}
          >
            <span>{item.label}</span>
            {item.hint && (
              <span style={{ fontSize: "var(--sk-text-xs)", opacity: 0.7 }}>{item.hint}</span>
            )}
          </button>
        ))}
        {filtered.length === 0 && (
          <div style={{ padding: 12, fontSize: "var(--sk-text-sm)", color: "var(--sk-color-text-muted)" }}>
            No matching commands.
          </div>
        )}
      </div>
    </Dialog>
  );
}

// =============================================================================
// ToastProvider & useToast
// =============================================================================

export interface ToastInput {
  title: string;
  description?: string;
  tone?: "default" | "success" | "danger";
}

interface ToastRecord extends ToastInput {
  id: string;
}

interface ToastContextValue {
  toast: (input: ToastInput) => void;
}

const ToastContext = createContext<ToastContextValue | null>(null);

export function ToastProvider({ children }: { children: ReactNode }) {
  const [toasts, setToasts] = useState<ToastRecord[]>([]);

  const toast = useCallback((input: ToastInput) => {
    const id = `${Date.now()}-${Math.random().toString(36).slice(2, 7)}`;
    setToasts((prev) => [...prev, { ...input, id }]);
  }, []);

  return (
    <ToastContext.Provider value={{ toast }}>
      <ToastPrimitive.Provider swipeDirection="right" duration={4000}>
        {children}
        {toasts.map((item) => (
          <ToastPrimitive.Root
            key={item.id}
            open
            onOpenChange={(open) => {
              if (!open) {
                setToasts((prev) => prev.filter((t) => t.id !== item.id));
              }
            }}
            className={clsx(
              styles.toastRoot,
              item.tone === "success" && styles.toastSuccess,
              item.tone === "danger" && styles.toastDanger,
            )}
          >
            <div>
              <ToastPrimitive.Title className={styles.toastTitle}>{item.title}</ToastPrimitive.Title>
              {item.description && (
                <ToastPrimitive.Description className={styles.toastDesc}>
                  {item.description}
                </ToastPrimitive.Description>
              )}
            </div>
            <ToastPrimitive.Close asChild>
              <IconButton label="Dismiss notification" size="sm">
                <X size={14} aria-hidden="true" />
              </IconButton>
            </ToastPrimitive.Close>
          </ToastPrimitive.Root>
        ))}
        <ToastPrimitive.Viewport className={styles.toastViewport} />
      </ToastPrimitive.Provider>
    </ToastContext.Provider>
  );
}

export function useToast(): ToastContextValue {
  const ctx = useContext(ToastContext);
  if (!ctx) {
    throw new Error("useToast must be used within a ToastProvider");
  }
  return ctx;
}
