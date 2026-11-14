// @ts-nocheck
/**
 * iOS shim for @expo/ui/jetpack-compose
 * Provides RN-based stub implementations so the app can run on iOS
 * without modifying screen code. Replace with proper @expo/ui/swift-ui
 * wrappers during the incremental migration.
 */

import React from 'react';
import {
  View,
  Text as RNText,
  Switch as RNSwitch,
  ActivityIndicator,
  TextInput,
  Modal,
  Pressable,
  StyleSheet,
  type ViewStyle,
  type StyleProp,
  type ColorValue,
} from 'react-native';

// ─── useNativeState ─────────────────────────────────────────────────

export function useNativeState<T>(initialValue: T): [T, (v: T) => void] {
  const [value, setValue] = React.useState(initialValue);
  React.useEffect(() => {
    setValue(initialValue);
  }, [initialValue]);
  return [value, setValue];
}

// ─── Host ───────────────────────────────────────────────────────────

export interface HostProps {
  matchContents?: boolean | { vertical?: boolean; horizontal?: boolean };
  style?: StyleProp<ViewStyle>;
  children?: React.ReactNode;
  [key: string]: any;
}

export function Host({ style, children }: HostProps) {
  return <View style={style}>{children}</View>;
}

// ─── Text ───────────────────────────────────────────────────────────

export interface TextProps {
  children?: string;
  color?: ColorValue;
  style?: any;
  modifiers?: any[];
  [key: string]: any;
}

export function Text({ children, color, style, modifiers, ...rest }: TextProps) {
  const onPress = modifiers?.find((m: any) => m?.type === 'clickable')?.onClick;
  return (
    <RNText
      style={{ color: color as string, ...(style?.typography ? {} : {}) }}
      onPress={onPress}
      {...rest}
    >
      {children}
    </RNText>
  );
}

// ─── Button Variants ────────────────────────────────────────────────

interface ButtonProps {
  onClick?: () => void;
  enabled?: boolean;
  colors?: { containerColor?: ColorValue; contentColor?: ColorValue };
  modifiers?: any[];
  children?: React.ReactNode;
  [key: string]: any;
}

function BaseButton({ onClick, enabled = true, colors, children }: ButtonProps) {
  return (
    <Pressable
      onPress={onClick}
      disabled={!enabled}
      style={[
        shimStyles.button,
        colors?.containerColor ? { backgroundColor: colors.containerColor as string } : null,
        !enabled && shimStyles.buttonDisabled,
      ]}
    >
      {React.Children.map(children, (child) => {
        if (
          React.isValidElement(child) &&
          (child.type === Text || (child.type as any)?.displayName === 'Text')
        ) {
          return React.cloneElement(child as React.ReactElement<any>, {
            color: (child.props as any).color || colors?.contentColor || '#fff',
          });
        }
        return child;
      })}
    </Pressable>
  );
}

export function Button(props: ButtonProps) {
  return <BaseButton {...props} />;
}

export function OutlinedButton({ colors, ...props }: ButtonProps) {
  return (
    <Pressable
      onPress={props.onClick}
      disabled={props.enabled === false}
      style={[shimStyles.outlinedButton, props.enabled === false && shimStyles.buttonDisabled]}
    >
      {React.Children.map(props.children, (child) => {
        if (
          React.isValidElement(child) &&
          (child.type === Text || (child.type as any)?.displayName === 'Text')
        ) {
          return React.cloneElement(child as React.ReactElement<any>, {
            color: (child.props as any).color || colors?.contentColor || '#007AFF',
          });
        }
        return child;
      })}
    </Pressable>
  );
}

export function FilledTonalButton({ colors, ...props }: ButtonProps) {
  return (
    <Pressable
      onPress={props.onClick}
      disabled={props.enabled === false}
      style={[
        shimStyles.tonalButton,
        colors?.containerColor ? { backgroundColor: colors.containerColor as string } : null,
        props.enabled === false && shimStyles.buttonDisabled,
      ]}
    >
      {React.Children.map(props.children, (child) => {
        if (
          React.isValidElement(child) &&
          (child.type === Text || (child.type as any)?.displayName === 'Text')
        ) {
          return React.cloneElement(child as React.ReactElement<any>, {
            color: (child.props as any).color || colors?.contentColor || '#333',
          });
        }
        return child;
      })}
    </Pressable>
  );
}

export function TextButton({ colors, ...props }: ButtonProps) {
  return (
    <Pressable
      onPress={props.onClick}
      disabled={props.enabled === false}
      style={props.enabled === false ? shimStyles.buttonDisabled : undefined}
    >
      {React.Children.map(props.children, (child) => {
        if (
          React.isValidElement(child) &&
          (child.type === Text || (child.type as any)?.displayName === 'Text')
        ) {
          return React.cloneElement(child as React.ReactElement<any>, {
            color: (child.props as any).color || colors?.contentColor || '#007AFF',
          });
        }
        return child;
      })}
    </Pressable>
  );
}

// ─── Switch ─────────────────────────────────────────────────────────

interface SwitchProps {
  value?: boolean;
  onCheckedChange?: (value: boolean) => void;
  enabled?: boolean;
  colors?: {
    checkedTrackColor?: ColorValue;
    uncheckedTrackColor?: ColorValue;
    checkedThumbColor?: ColorValue;
    uncheckedThumbColor?: ColorValue;
    [key: string]: any;
  };
  [key: string]: any;
}

export function Switch({ value, onCheckedChange, enabled = true, colors }: SwitchProps) {
  return (
    <RNSwitch
      value={value}
      onValueChange={onCheckedChange}
      disabled={!enabled}
      trackColor={{
        false: colors?.uncheckedTrackColor as string,
        true: colors?.checkedTrackColor as string,
      }}
      thumbColor={
        value ? (colors?.checkedThumbColor as string) : (colors?.uncheckedThumbColor as string)
      }
    />
  );
}

// ─── Progress Indicators ────────────────────────────────────────────

interface ProgressProps {
  color?: ColorValue;
  modifiers?: any[];
  [key: string]: any;
}

export function CircularProgressIndicator({ color }: ProgressProps) {
  return <ActivityIndicator color={color as string} />;
}

interface LinearProgressProps {
  progress?: number;
  color?: ColorValue;
  trackColor?: ColorValue;
  modifiers?: any[];
  [key: string]: any;
}

export function LinearProgressIndicator({ progress, color, trackColor }: LinearProgressProps) {
  const pct = progress != null ? Math.max(0, Math.min(1, progress)) : undefined;
  return (
    <View
      style={[
        shimStyles.linearTrack,
        trackColor ? { backgroundColor: trackColor as string } : null,
      ]}
    >
      {pct != null ? (
        <View
          style={[
            shimStyles.linearFill,
            { width: `${pct * 100}%`, backgroundColor: (color as string) || '#007AFF' },
          ]}
        />
      ) : (
        <ActivityIndicator color={color as string} style={{ alignSelf: 'center' }} />
      )}
    </View>
  );
}

// ─── AlertDialog ────────────────────────────────────────────────────

interface AlertDialogProps {
  onDismissRequest?: () => void;
  colors?: { containerColor?: ColorValue };
  children?: React.ReactNode;
  [key: string]: any;
}

function AlertDialogTitle({ children }: { children?: React.ReactNode }) {
  return <>{children}</>;
}
function AlertDialogText({ children }: { children?: React.ReactNode }) {
  return <>{children}</>;
}
function AlertDialogConfirmButton({ children }: { children?: React.ReactNode }) {
  return <>{children}</>;
}
function AlertDialogDismissButton({ children }: { children?: React.ReactNode }) {
  return <>{children}</>;
}

export function AlertDialog({ onDismissRequest, colors, children }: AlertDialogProps) {
  let title = '';
  let message = '';
  let confirmNode: React.ReactNode = null;
  let dismissNode: React.ReactNode = null;

  React.Children.forEach(children, (child) => {
    if (!React.isValidElement(child)) return;
    if (child.type === AlertDialogTitle || (child.type as any) === AlertDialog.Title) {
      React.Children.forEach(child.props.children, (c: any) => {
        if (typeof c === 'string') title = c;
        else if (React.isValidElement(c) && (c.props as any).children)
          title = (c.props as any).children;
      });
    } else if (child.type === AlertDialogText || (child.type as any) === AlertDialog.Text) {
      React.Children.forEach(child.props.children, (c: any) => {
        if (typeof c === 'string') message = c;
        else if (React.isValidElement(c) && (c.props as any).children)
          message = (c.props as any).children;
      });
    } else if (
      child.type === AlertDialogConfirmButton ||
      (child.type as any) === AlertDialog.ConfirmButton
    ) {
      confirmNode = child.props.children;
    } else if (
      child.type === AlertDialogDismissButton ||
      (child.type as any) === AlertDialog.DismissButton
    ) {
      dismissNode = child.props.children;
    }
  });

  return (
    <Modal transparent animationType="fade" onRequestClose={onDismissRequest}>
      <Pressable style={shimStyles.dialogOverlay} onPress={onDismissRequest}>
        <View
          style={[
            shimStyles.dialogContainer,
            colors?.containerColor ? { backgroundColor: colors.containerColor as string } : null,
          ]}
        >
          {title ? <RNText style={shimStyles.dialogTitle}>{title}</RNText> : null}
          {message ? <RNText style={shimStyles.dialogMessage}>{message}</RNText> : null}
          <View style={shimStyles.dialogButtons}>
            {dismissNode && <View style={shimStyles.dialogBtn}>{dismissNode}</View>}
            {confirmNode && <View style={shimStyles.dialogBtn}>{confirmNode}</View>}
          </View>
        </View>
      </Pressable>
    </Modal>
  );
}
AlertDialog.Title = AlertDialogTitle;
AlertDialog.Text = AlertDialogText;
AlertDialog.ConfirmButton = AlertDialogConfirmButton;
AlertDialog.DismissButton = AlertDialogDismissButton;

// ─── OutlinedTextField ──────────────────────────────────────────────

interface OutlinedTextFieldProps {
  defaultValue?: string;
  onValueChange?: (value: string) => void;
  onFocusChanged?: (focused: boolean) => void;
  keyboardOptions?: any;
  singleLine?: boolean;
  readOnly?: boolean;
  modifiers?: any[];
  colors?: any;
  children?: React.ReactNode;
  [key: string]: any;
}

function TextFieldPlaceholder({ children }: { children?: React.ReactNode }) {
  return <>{children}</>;
}
function TextFieldSuffix({ children }: { children?: React.ReactNode }) {
  return <>{children}</>;
}

export function OutlinedTextField({
  defaultValue,
  onValueChange,
  onFocusChanged,
  keyboardOptions,
  singleLine,
  readOnly,
  colors,
  children,
}: OutlinedTextFieldProps) {
  let placeholder = '';
  React.Children.forEach(children, (child) => {
    if (
      React.isValidElement(child) &&
      ((child.type as any) === OutlinedTextField.Placeholder ||
        (child.type as any) === TextFieldPlaceholder)
    ) {
      React.Children.forEach(child.props.children, (c: any) => {
        if (typeof c === 'string') placeholder = c;
        else if (React.isValidElement(c) && (c.props as any).children)
          placeholder = (c.props as any).children;
      });
    }
  });

  const keyboardType =
    keyboardOptions?.keyboardType === 'number' ? ('numeric' as const) : ('default' as const);

  return (
    <TextInput
      defaultValue={defaultValue}
      onChangeText={onValueChange}
      onFocus={() => onFocusChanged?.(true)}
      onBlur={() => onFocusChanged?.(false)}
      placeholder={placeholder}
      editable={!readOnly}
      multiline={!singleLine}
      keyboardType={keyboardType}
      style={[
        shimStyles.textField,
        colors?.focusedTextColor ? { color: colors.focusedTextColor as string } : null,
      ]}
      placeholderTextColor={colors?.unfocusedPlaceholderColor as string}
    />
  );
}
OutlinedTextField.Placeholder = TextFieldPlaceholder;
OutlinedTextField.Suffix = TextFieldSuffix;

// ─── Card ───────────────────────────────────────────────────────────

interface CardProps {
  colors?: { containerColor?: ColorValue };
  children?: React.ReactNode;
  [key: string]: any;
}

export function Card({ colors, children }: CardProps) {
  return (
    <View
      style={[
        shimStyles.card,
        colors?.containerColor ? { backgroundColor: colors.containerColor as string } : null,
      ]}
    >
      {children}
    </View>
  );
}

// ─── ListItem ───────────────────────────────────────────────────────

interface ListItemProps {
  colors?: { containerColor?: ColorValue };
  children?: React.ReactNode;
  [key: string]: any;
}

function ListItemOverlineContent({ children }: { children?: React.ReactNode }) {
  return <View>{children}</View>;
}
function ListItemHeadlineContent({ children }: { children?: React.ReactNode }) {
  return <View>{children}</View>;
}
function ListItemSupportingContent({ children }: { children?: React.ReactNode }) {
  return <View>{children}</View>;
}
function ListItemTrailingContent({ children }: { children?: React.ReactNode }) {
  return <View>{children}</View>;
}

export function ListItem({ colors, children }: ListItemProps) {
  let overline: React.ReactNode = null;
  let headline: React.ReactNode = null;
  let supporting: React.ReactNode = null;
  let trailing: React.ReactNode = null;

  React.Children.forEach(children, (child) => {
    if (!React.isValidElement(child)) return;
    const type = child.type as any;
    if (type === ListItemOverlineContent || type === ListItem.OverlineContent)
      overline = child.props.children;
    else if (type === ListItemHeadlineContent || type === ListItem.HeadlineContent)
      headline = child.props.children;
    else if (type === ListItemSupportingContent || type === ListItem.SupportingContent)
      supporting = child.props.children;
    else if (type === ListItemTrailingContent || type === ListItem.TrailingContent)
      trailing = child.props.children;
  });

  return (
    <View
      style={[
        shimStyles.listItem,
        colors?.containerColor ? { backgroundColor: colors.containerColor as string } : null,
      ]}
    >
      <View style={{ flex: 1 }}>
        {overline}
        {headline}
        {supporting}
      </View>
      {trailing && <View>{trailing}</View>}
    </View>
  );
}
ListItem.OverlineContent = ListItemOverlineContent;
ListItem.HeadlineContent = ListItemHeadlineContent;
ListItem.SupportingContent = ListItemSupportingContent;
ListItem.TrailingContent = ListItemTrailingContent;

// ─── Layout: Column, Row, Spacer ────────────────────────────────────

interface ColumnProps {
  modifiers?: any[];
  children?: React.ReactNode;
  [key: string]: any;
}

export function Column({ children }: ColumnProps) {
  return <View style={{ flexDirection: 'column', width: '100%' }}>{children}</View>;
}

interface RowProps {
  verticalAlignment?: string;
  horizontalArrangement?: string;
  modifiers?: any[];
  children?: React.ReactNode;
  [key: string]: any;
}

export function Row({ verticalAlignment, horizontalArrangement, children }: RowProps) {
  return (
    <View
      style={{
        flexDirection: 'row',
        alignItems: verticalAlignment === 'center' ? 'center' : undefined,
        justifyContent:
          horizontalArrangement === 'center'
            ? 'center'
            : horizontalArrangement === 'end'
            ? 'flex-end'
            : undefined,
        width: '100%',
      }}
    >
      {children}
    </View>
  );
}

interface SpacerProps {
  modifiers?: any[];
  [key: string]: any;
}

export function Spacer({ modifiers }: SpacerProps) {
  let w: number | undefined;
  let h: number | undefined;
  modifiers?.forEach((m: any) => {
    if (m?.type === 'width') w = m.value;
    if (m?.type === 'height') h = m.value;
  });
  return <View style={{ width: w, height: h }} />;
}

// ─── HorizontalDivider ──────────────────────────────────────────────

interface DividerProps {
  color?: ColorValue;
  [key: string]: any;
}

export function HorizontalDivider({ color }: DividerProps) {
  return <View style={[shimStyles.divider, color ? { backgroundColor: color as string } : null]} />;
}

// ─── ModalBottomSheet ───────────────────────────────────────────────

interface ModalBottomSheetProps {
  onDismissRequest?: () => void;
  children?: React.ReactNode;
  [key: string]: any;
}

export function ModalBottomSheet({ onDismissRequest, children }: ModalBottomSheetProps) {
  return (
    <Modal transparent animationType="slide" onRequestClose={onDismissRequest}>
      <Pressable style={shimStyles.bottomSheetOverlay} onPress={onDismissRequest}>
        <View style={shimStyles.bottomSheetContent} onStartShouldSetResponder={() => true}>
          {children}
        </View>
      </Pressable>
    </Modal>
  );
}

// ─── Dropdown (ExposedDropdownMenuBox) ──────────────────────────────

interface ExposedDropdownMenuBoxProps {
  expanded?: boolean;
  onExpandedChange?: (expanded: boolean) => void;
  modifiers?: any[];
  children?: React.ReactNode;
  [key: string]: any;
}

export function ExposedDropdownMenuBox({ children }: ExposedDropdownMenuBoxProps) {
  return <View>{children}</View>;
}

interface ExposedDropdownMenuProps {
  expanded?: boolean;
  onDismissRequest?: () => void;
  children?: React.ReactNode;
  [key: string]: any;
}

export function ExposedDropdownMenu({
  expanded,
  onDismissRequest,
  children,
}: ExposedDropdownMenuProps) {
  if (!expanded) return null;
  return (
    <Modal transparent animationType="fade" onRequestClose={onDismissRequest}>
      <Pressable style={shimStyles.dropdownOverlay} onPress={onDismissRequest}>
        <View style={shimStyles.dropdownMenu}>{children}</View>
      </Pressable>
    </Modal>
  );
}

function DropdownMenuItemText({ children }: { children?: React.ReactNode }) {
  return <>{children}</>;
}

interface DropdownMenuItemProps {
  onClick?: () => void;
  children?: React.ReactNode;
  [key: string]: any;
}

export function DropdownMenuItem({ onClick, children }: DropdownMenuItemProps) {
  let label = '';
  React.Children.forEach(children, (child) => {
    if (!React.isValidElement(child)) return;
    if (
      (child.type as any) === DropdownMenuItem.Text ||
      (child.type as any) === DropdownMenuItemText
    ) {
      React.Children.forEach(child.props.children, (c: any) => {
        if (typeof c === 'string') label = c;
        else if (React.isValidElement(c) && (c.props as any).children)
          label = (c.props as any).children;
      });
    }
  });
  return (
    <Pressable onPress={onClick} style={shimStyles.dropdownItem}>
      <RNText>{label}</RNText>
    </Pressable>
  );
}
DropdownMenuItem.Text = DropdownMenuItemText;

// ─── Styles ─────────────────────────────────────────────────────────

const shimStyles = StyleSheet.create({
  button: {
    backgroundColor: '#007AFF',
    paddingVertical: 12,
    paddingHorizontal: 16,
    borderRadius: 8,
    alignItems: 'center',
  },
  outlinedButton: {
    borderWidth: 1,
    borderColor: '#007AFF',
    paddingVertical: 12,
    paddingHorizontal: 16,
    borderRadius: 8,
    alignItems: 'center',
  },
  tonalButton: {
    backgroundColor: '#E8E8E8',
    paddingVertical: 12,
    paddingHorizontal: 16,
    borderRadius: 8,
    alignItems: 'center',
  },
  buttonDisabled: {
    opacity: 0.5,
  },
  linearTrack: {
    height: 4,
    backgroundColor: '#E0E0E0',
    borderRadius: 2,
    width: '100%',
    overflow: 'hidden',
  },
  linearFill: {
    height: '100%',
    borderRadius: 2,
  },
  textField: {
    borderWidth: 1,
    borderColor: '#ccc',
    borderRadius: 4,
    paddingHorizontal: 12,
    paddingVertical: 10,
    fontSize: 16,
    width: '100%',
  },
  card: {
    backgroundColor: '#fff',
    borderRadius: 12,
    overflow: 'hidden',
  },
  listItem: {
    flexDirection: 'row',
    alignItems: 'center',
    paddingVertical: 12,
    paddingHorizontal: 16,
  },
  divider: {
    height: StyleSheet.hairlineWidth,
    backgroundColor: '#E0E0E0',
    width: '100%',
  },
  dialogOverlay: {
    flex: 1,
    backgroundColor: 'rgba(0,0,0,0.5)',
    justifyContent: 'center',
    alignItems: 'center',
  },
  dialogContainer: {
    backgroundColor: '#fff',
    borderRadius: 14,
    padding: 20,
    width: '80%',
    maxWidth: 320,
  },
  dialogTitle: {
    fontSize: 17,
    fontWeight: '600',
    marginBottom: 8,
    textAlign: 'center',
  },
  dialogMessage: {
    fontSize: 15,
    color: '#666',
    marginBottom: 16,
    textAlign: 'center',
  },
  dialogButtons: {
    flexDirection: 'row',
    justifyContent: 'flex-end',
    gap: 12,
  },
  dialogBtn: {
    paddingVertical: 6,
    paddingHorizontal: 12,
  },
  bottomSheetOverlay: {
    flex: 1,
    backgroundColor: 'rgba(0,0,0,0.3)',
    justifyContent: 'flex-end',
  },
  bottomSheetContent: {
    backgroundColor: '#fff',
    borderTopLeftRadius: 16,
    borderTopRightRadius: 16,
    padding: 16,
    maxHeight: '80%',
  },
  dropdownOverlay: {
    flex: 1,
    backgroundColor: 'rgba(0,0,0,0.1)',
    justifyContent: 'center',
    alignItems: 'center',
  },
  dropdownMenu: {
    backgroundColor: '#fff',
    borderRadius: 8,
    paddingVertical: 4,
    minWidth: 150,
    shadowColor: '#000',
    shadowOffset: { width: 0, height: 2 },
    shadowOpacity: 0.25,
    shadowRadius: 4,
    elevation: 5,
  },
  dropdownItem: {
    paddingVertical: 12,
    paddingHorizontal: 16,
  },
});
