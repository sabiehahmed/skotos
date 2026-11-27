import clsx from "clsx";
import { AlertCircle, AlertTriangle, Check, CheckCircle2, Copy, Info } from "lucide-react";
import { useState, type CSSProperties, type HTMLAttributes, type ReactNode } from "react";
import styles from "./Typography.module.css";

export interface TextProps extends HTMLAttributes<HTMLParagraphElement> {
  as?: "p" | "span" | "div";
  size?: "xs" | "sm" | "md" | "lg";
  tone?: "default" | "secondary" | "muted" | "danger" | "success";
  mono?: boolean;
  bold?: boolean;
}

export function Text({
  as: Tag = "p",
  size = "md",
  tone = "default",
  mono = false,
  bold = false,
  className,
  children,
  ...props
}: TextProps) {
  return (
    <Tag
      className={clsx(
        styles.text,
        size === "xs" && styles.textXs,
        size === "sm" && styles.textSm,
        size === "lg" && styles.textLg,
        tone === "muted" && styles.textMuted,
        tone === "secondary" && styles.textSecondary,
        tone === "danger" && styles.textDanger,
        tone === "success" && styles.textSuccess,
        mono && styles.textMono,
        bold && styles.textBold,
        className,
      )}
      {...props}
    >
      {children}
    </Tag>
  );
}

export interface HeadingProps extends HTMLAttributes<HTMLHeadingElement> {
  level?: 1 | 2 | 3 | 4;
}

export function Heading({ level = 2, className, children, ...props }: HeadingProps) {
  const Tag = `h${level}` as const;
  return (
    <Tag
      className={clsx(
        styles.heading,
        level === 1 && styles.h1,
        level === 2 && styles.h2,
        level === 3 && styles.h3,
        level === 4 && styles.h4,
        className,
      )}
      {...props}
    >
      {children}
    </Tag>
  );
}

export interface CodeBlockProps {
  code: string;
  label?: string;
  copyable?: boolean;
  className?: string;
}

export function CodeBlock({ code, label, copyable = true, className }: CodeBlockProps) {
  const [copied, setCopied] = useState(false);

  const handleCopy = async () => {
    try {
      await navigator.clipboard?.writeText(code);
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1500);
    } catch {
      // Clipboard may be unavailable in non-secure environments
    }
  };

  return (
    <div className={clsx(styles.codeBlockWrap, className)}>
      {(label || copyable) && (
        <div className={styles.codeBlockHeader}>
          <span>{label ?? "Code"}</span>
          {copyable && (
            <button type="button" className={styles.copyBtn} onClick={handleCopy}>
              {copied ? <Check size={12} /> : <Copy size={12} />}
              {copied ? "Copied" : "Copy"}
            </button>
          )}
        </div>
      )}
      <pre className={styles.codeBlockPre}>
        <code>{code}</code>
      </pre>
    </div>
  );
}

export interface BadgeProps extends HTMLAttributes<HTMLSpanElement> {
  tone?: "neutral" | "accent" | "success" | "warning" | "danger";
}

export function Badge({ tone = "neutral", className, children, ...props }: BadgeProps) {
  return (
    <span
      className={clsx(
        styles.badge,
        tone === "neutral" && styles.badgeNeutral,
        tone === "accent" && styles.badgeAccent,
        tone === "success" && styles.badgeSuccess,
        tone === "warning" && styles.badgeWarning,
        tone === "danger" && styles.badgeDanger,
        className,
      )}
      {...props}
    >
      {children}
    </span>
  );
}

export interface StatusDotProps {
  tone?: "success" | "warning" | "danger" | "neutral";
  color?: string;
  label?: string;
}

export function StatusDot({ tone = "neutral", color, label }: StatusDotProps) {
  return (
    <span
      role={label ? "img" : undefined}
      aria-label={label}
      aria-hidden={!label}
      className={clsx(
        styles.statusDot,
        !color && tone === "success" && styles.dotSuccess,
        !color && tone === "warning" && styles.dotWarning,
        !color && tone === "danger" && styles.dotDanger,
        !color && tone === "neutral" && styles.dotNeutral,
      )}
      style={color ? { backgroundColor: color } : undefined}
    />
  );
}

export interface AlertProps extends HTMLAttributes<HTMLDivElement> {
  tone?: "info" | "success" | "warning" | "danger";
  title?: string;
  children: ReactNode;
}

export function Alert({ tone = "info", title, children, className, ...props }: AlertProps) {
  const Icon =
    tone === "success"
      ? CheckCircle2
      : tone === "warning"
        ? AlertTriangle
        : tone === "danger"
          ? AlertCircle
          : Info;

  return (
    <div
      role={tone === "danger" || tone === "warning" ? "alert" : "status"}
      className={clsx(
        styles.alert,
        tone === "info" && styles.alertInfo,
        tone === "success" && styles.alertSuccess,
        tone === "warning" && styles.alertWarning,
        tone === "danger" && styles.alertDanger,
        className,
      )}
      {...props}
    >
      <Icon size={16} aria-hidden="true" style={{ flexShrink: 0, marginTop: 2 }} />
      <div>
        {title && <div className={styles.alertTitle}>{title}</div>}
        <div>{children}</div>
      </div>
    </div>
  );
}

export interface SkeletonProps {
  width?: string | number;
  height?: string | number;
  className?: string;
}

export function Skeleton({ width = "100%", height = 20, className }: SkeletonProps) {
  const style: CSSProperties = { width, height };
  return <div aria-hidden="true" className={clsx(styles.skeleton, className)} style={style} />;
}

export interface EmptyStateProps {
  icon?: ReactNode;
  title: string;
  description?: string;
  action?: ReactNode;
  headingLevel?: 2 | 3 | 4;
  className?: string;
}

export function EmptyState({
  icon,
  title,
  description,
  action,
  headingLevel = 2,
  className,
}: EmptyStateProps) {
  return (
    <div className={clsx(styles.emptyState, className)}>
      {icon && <div className={styles.emptyIcon}>{icon}</div>}
      <Heading level={headingLevel}>{title}</Heading>
      {description && (
        <Text size="sm" tone="muted">
          {description}
        </Text>
      )}
      {action && <div style={{ marginTop: "var(--sk-space-2)" }}>{action}</div>}
    </div>
  );
}

export interface SpinnerProps {
  size?: number;
  label?: string;
}

export function Spinner({ size = 18, label = "Loading" }: SpinnerProps) {
  return (
    <span
      role="status"
      aria-label={label}
      className={styles.spinner}
      style={{ width: size, height: size }}
    />
  );
}
