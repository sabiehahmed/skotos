import * as SeparatorPrimitive from "@radix-ui/react-separator";
import clsx from "clsx";
import type { CSSProperties, HTMLAttributes, ReactNode } from "react";
import styles from "./Layout.module.css";

export interface AppShellProps {
  sidebar?: ReactNode;
  topBar?: ReactNode;
  banner?: ReactNode;
  envColor?: string;
  children: ReactNode;
  className?: string;
}

export function AppShell({
  sidebar,
  topBar,
  banner,
  envColor,
  children,
  className,
}: AppShellProps) {
  const inlineStyle = envColor
    ? ({ "--sk-env-accent": envColor } as CSSProperties)
    : undefined;

  return (
    <div
      className={clsx(styles.appShell, !sidebar && styles.appShellNoSidebar, className)}
      style={inlineStyle}
    >
      {sidebar}
      <div className={styles.mainColumn}>
        {topBar}
        {banner}
        <main className={styles.mainContent}>{children}</main>
      </div>
    </div>
  );
}

export interface SidebarProps extends HTMLAttributes<HTMLElement> {
  header?: ReactNode;
  footer?: ReactNode;
  children: ReactNode;
}

export function Sidebar({ header, footer, children, className, ...props }: SidebarProps) {
  return (
    <aside className={clsx(styles.sidebar, className)} {...props}>
      {header && <div className={styles.sidebarHeader}>{header}</div>}
      <nav className={styles.sidebarContent} aria-label="Main navigation">
        {children}
      </nav>
      {footer && <div className={styles.sidebarFooter}>{footer}</div>}
    </aside>
  );
}

export interface TopBarProps extends HTMLAttributes<HTMLElement> {
  left?: ReactNode;
  right?: ReactNode;
}

export function TopBar({ left, right, className, ...props }: TopBarProps) {
  return (
    <header className={clsx(styles.topBar, className)} {...props}>
      <div className={styles.topBarSection}>{left}</div>
      <div className={styles.topBarSection}>{right}</div>
    </header>
  );
}

export interface EnvironmentBannerProps {
  environmentName: string;
  environmentKey: string;
  color: string;
  protectedEnv?: boolean;
  rightSlot?: ReactNode;
}

export function EnvironmentBanner({
  environmentName,
  environmentKey,
  color,
  protectedEnv = false,
  rightSlot,
}: EnvironmentBannerProps) {
  return (
    <div
      role="status"
      data-testid="environment-banner"
      data-env-key={environmentKey}
      className={styles.envBanner}
      style={{ "--sk-env-accent": color } as CSSProperties}
    >
      <div className={styles.envBannerLeft}>
        <span className={styles.envBannerSwatch} aria-hidden="true" />
        <span>
          Environment: {environmentName} ({environmentKey})
        </span>
        {protectedEnv && <span>— Protected: review & confirmation enabled</span>}
      </div>
      {rightSlot && <div>{rightSlot}</div>}
    </div>
  );
}

export interface PageHeaderProps {
  title: ReactNode;
  description?: ReactNode;
  breadcrumbs?: ReactNode;
  actions?: ReactNode;
  className?: string;
}

export function PageHeader({
  title,
  description,
  breadcrumbs,
  actions,
  className,
}: PageHeaderProps) {
  return (
    <div className={clsx(styles.pageHeader, className)}>
      <div className={styles.pageHeaderTitles}>
        {breadcrumbs}
        <div>{title}</div>
        {description && <div>{description}</div>}
      </div>
      {actions && <div className={styles.pageHeaderActions}>{actions}</div>}
    </div>
  );
}

export interface CardProps extends HTMLAttributes<HTMLDivElement> {
  padded?: boolean;
  header?: ReactNode;
}

export function Card({ padded = false, header, children, className, ...props }: CardProps) {
  return (
    <div className={clsx(styles.card, padded && !header && styles.cardPadded, className)} {...props}>
      {header && <div className={styles.cardHeader}>{header}</div>}
      {header ? <div className={styles.cardBody}>{children}</div> : children}
    </div>
  );
}

export interface StackProps extends HTMLAttributes<HTMLDivElement> {
  direction?: "vertical" | "horizontal";
  gap?: 1 | 2 | 3 | 4 | 5 | 6 | 8;
  align?: CSSProperties["alignItems"];
  justify?: CSSProperties["justifyContent"];
}

export function Stack({
  direction = "vertical",
  gap = 3,
  align,
  justify,
  style,
  className,
  children,
  ...props
}: StackProps) {
  return (
    <div
      className={clsx(
        styles.stack,
        direction === "vertical" ? styles.stackVertical : styles.stackHorizontal,
        className,
      )}
      style={{
        gap: `var(--sk-space-${gap})`,
        alignItems: align,
        justifyContent: justify,
        ...style,
      }}
      {...props}
    >
      {children}
    </div>
  );
}

export interface GridProps extends HTMLAttributes<HTMLDivElement> {
  columns?: number | string;
  gap?: 1 | 2 | 3 | 4 | 5 | 6 | 8;
}

export function Grid({ columns = 2, gap = 4, style, className, children, ...props }: GridProps) {
  const template = typeof columns === "number" ? `repeat(${columns}, minmax(0, 1fr))` : columns;
  return (
    <div
      className={clsx(styles.grid, className)}
      style={{
        gridTemplateColumns: template,
        gap: `var(--sk-space-${gap})`,
        ...style,
      }}
      {...props}
    >
      {children}
    </div>
  );
}

export interface SplitPaneProps {
  primary: ReactNode;
  secondary: ReactNode;
  className?: string;
}

export function SplitPane({ primary, secondary, className }: SplitPaneProps) {
  return (
    <div className={clsx(styles.splitPane, className)}>
      <div>{primary}</div>
      <div>{secondary}</div>
    </div>
  );
}

export interface SeparatorProps {
  orientation?: "horizontal" | "vertical";
  decorative?: boolean;
  className?: string;
}

export function Separator({
  orientation = "horizontal",
  decorative = true,
  className,
}: SeparatorProps) {
  return (
    <SeparatorPrimitive.Root
      orientation={orientation}
      decorative={decorative}
      className={clsx(styles.separator, className)}
    />
  );
}
