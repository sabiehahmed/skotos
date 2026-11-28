import "./styles/index.css";

// Theme
export {
  ThemeProvider,
  useTheme,
  type ResolvedTheme,
  type ThemeContextValue,
  type ThemeMode,
  type ThemeProviderProps,
} from "./theme/ThemeProvider";

// Layout & Shell (10)
export {
  AppShell,
  Card,
  EnvironmentBanner,
  Grid,
  PageHeader,
  Separator,
  Sidebar,
  SplitPane,
  Stack,
  TopBar,
  type AppShellProps,
  type CardProps,
  type EnvironmentBannerProps,
  type GridProps,
  type PageHeaderProps,
  type SeparatorProps,
  type SidebarProps,
  type SplitPaneProps,
  type StackProps,
  type TopBarProps,
} from "./components/layout/Layout";

// Typography & Feedback (9)
export {
  Alert,
  Badge,
  CodeBlock,
  EmptyState,
  Heading,
  Skeleton,
  Spinner,
  StatusDot,
  Text,
  type AlertProps,
  type BadgeProps,
  type CodeBlockProps,
  type EmptyStateProps,
  type HeadingProps,
  type SkeletonProps,
  type SpinnerProps,
  type StatusDotProps,
  type TextProps,
} from "./components/typography/Typography";

// Forms & Controls (15)
export {
  Button,
  Checkbox,
  Combobox,
  FormField,
  IconButton,
  Input,
  KeyValueEditor,
  Label,
  Select,
  Slider,
  SuggestInput,
  Switch,
  TagInput,
  Textarea,
  ToggleGroup,
  type ButtonProps,
  type CheckboxProps,
  type ComboboxProps,
  type FormFieldProps,
  type IconButtonProps,
  type InputProps,
  type KeyValueEditorProps,
  type KeyValuePair,
  type LabelProps,
  type SelectOption,
  type SelectProps,
  type SliderProps,
  type SuggestInputProps,
  type SuggestOption,
  type SwitchProps,
  type TagInputProps,
  type TextareaProps,
  type ToggleGroupItemOption,
  type ToggleGroupProps,
} from "./components/forms/Forms";

// Overlays & Navigation (11)
export {
  AlertDialog,
  Breadcrumb,
  CommandPalette,
  Dialog,
  Drawer,
  DropdownMenu,
  Pagination,
  Popover,
  Tabs,
  ToastProvider,
  Tooltip,
  TooltipProvider,
  useToast,
  type AlertDialogProps,
  type BreadcrumbItem,
  type BreadcrumbProps,
  type CommandItem,
  type CommandPaletteProps,
  type DialogProps,
  type DrawerProps,
  type DropdownMenuItem,
  type DropdownMenuProps,
  type PaginationProps,
  type PopoverProps,
  type TabItem,
  type TabsProps,
  type ToastInput,
  type TooltipProps,
} from "./components/overlays/Overlays";

// Data & Domain Widgets (6)
export {
  AuditTimeline,
  CANONICAL_CLAUSE_OPS,
  DataTable,
  DiffViewer,
  JsonEditor,
  DotMeter,
  RolloutAllocator,
  BUILT_IN_CONTEXT_KINDS,
  RuleBuilder,
  builtInAttributeSuggestions,
  type AuditTimelineEntry,
  type AuditTimelineProps,
  type DataTableColumn,
  type DataTableProps,
  type DiffInstructionSummary,
  type DiffViewerProps,
  type JsonEditorProps,
  type DotMeterProps,
  type RolloutAllocatorProps,
  type RolloutValue,
  type RolloutVariationOption,
  type RolloutWeightedVariation,
  type RuleBuilderClause,
  type RuleBuilderProps,
  type RuleBuilderRule,
} from "./components/data/Data";

// Curated Icons (encapsulated inside @skotos/ui)
export * as Icons from "./icons";

export const UI_LIBRARY_NAME = "@skotos/ui";

// Legacy M1 compatibility export
export { SkotosShellStub, type SkotosShellStubProps } from "./SkotosShellStub";
