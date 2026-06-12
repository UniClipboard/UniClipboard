/**
 * Word Picker Screen
 * 分词选择页面：从底部弹出的面板，将文本按词/字拆分，用户可选择多个词，然后复制选中内容。
 */

import React, { useState, useMemo, useCallback, useEffect, useRef } from 'react';
import {
  View,
  Text,
  StyleSheet,
  TouchableOpacity,
  ScrollView,
  ToastAndroid,
  Animated,
  Pressable,
  Dimensions,
  Easing,
  BackHandler,
  Switch,
} from 'react-native';
import * as Clipboard from 'expo-clipboard';
import { Segment, useDefault } from 'segmentit';
import { useTheme } from '@/hooks/useTheme';

const SCREEN_HEIGHT = Dimensions.get('window').height;
const SHEET_HEIGHT = SCREEN_HEIGHT * 0.75;

let segmentInstance: Segment | null = null;

function getSegment(): Segment {
  if (!segmentInstance) {
    segmentInstance = useDefault(new Segment());
  }
  return segmentInstance;
}

interface WordPickerScreenProps {
  text: string;
  onComplete: () => void;
}

/**
 * 将文本拆分为 token 数组。
 * - 使用 segmentit 对中文进行基于词组的分词
 * - 同时支持英文单词、标点、URL 等的识别
 * - 空白字符单独保留（用于渲染间距，不可选）
 */
function tokenize(text: string): string[] {
  const seg = getSegment();
  const result = seg.doSegment(text);
  return result.map((token) => token.w);
}

/**
 * 将文本按单个字符拆分。
 * - 拉丁字母/数字按连续序列（单词）拆分
 * - 其余字符（含CJK）逐字拆分
 */
function tokenizeByChar(text: string): string[] {
  const regex = /[a-zA-Z0-9]+|[\s]+|./g;
  return text.match(regex) || [];
}

/**
 * 计算每个 token 在原始文本中的字符位置范围 [start, end)
 */
function tokenPositions(tokens: string[]): Array<[number, number]> {
  const positions: Array<[number, number]> = [];
  let offset = 0;
  for (const t of tokens) {
    positions.push([offset, offset + t.length]);
    offset += t.length;
  }
  return positions;
}

export const WordPickerScreen: React.FC<WordPickerScreenProps> = ({ text, onComplete }) => {
  const { theme } = useTheme();
  const [splitAll, setSplitAll] = useState(false);
  const tokens = useMemo(
    () => (splitAll ? tokenizeByChar(text) : tokenize(text)),
    [text, splitAll]
  );
  const [selected, setSelected] = useState<Set<number>>(new Set());
  const prevTokensRef = useRef<string[]>(tokens);
  const prevSelectedRef = useRef<Set<number>>(selected);

  // 保持 ref 与最新 selected 同步
  useEffect(() => {
    prevSelectedRef.current = selected;
  }, [selected]);

  // 切换分词模式时，将旧选中映射到新 tokens
  useEffect(() => {
    const prevTokens = prevTokensRef.current;
    const prevSelected = prevSelectedRef.current;
    prevTokensRef.current = tokens;

    if (prevSelected.size === 0) return;

    // 计算旧选中 tokens 覆盖的字符位置集合
    const oldPositions = tokenPositions(prevTokens);
    const selectedChars = new Set<number>();
    for (const idx of prevSelected) {
      if (idx < oldPositions.length) {
        const [start, end] = oldPositions[idx];
        for (let c = start; c < end; c++) {
          selectedChars.add(c);
        }
      }
    }

    // 在新 tokens 中找出包含这些字符位置的索引
    const newPositions = tokenPositions(tokens);
    const newSelected = new Set<number>();
    newPositions.forEach(([start, end], i) => {
      // 只要 token 中有一个字符被选中，则该 token 选中
      for (let c = start; c < end; c++) {
        if (selectedChars.has(c)) {
          newSelected.add(i);
          break;
        }
      }
    });

    setSelected(newSelected);
  }, [tokens]);

  const slideAnim = useRef(new Animated.Value(SHEET_HEIGHT)).current;
  const backdropAnim = useRef(new Animated.Value(0)).current;

  const close = useCallback(() => {
    Animated.parallel([
      Animated.timing(slideAnim, {
        toValue: SHEET_HEIGHT,
        duration: 250,
        easing: Easing.in(Easing.cubic),
        useNativeDriver: true,
      }),
      Animated.timing(backdropAnim, {
        toValue: 0,
        duration: 250,
        useNativeDriver: true,
      }),
    ]).start(({ finished }) => {
      if (finished) onComplete();
    });
  }, [slideAnim, backdropAnim, onComplete]);

  useEffect(() => {
    Animated.parallel([
      Animated.timing(slideAnim, {
        toValue: 0,
        duration: 300,
        easing: Easing.out(Easing.cubic),
        useNativeDriver: true,
      }),
      Animated.timing(backdropAnim, {
        toValue: 1,
        duration: 300,
        useNativeDriver: true,
      }),
    ]).start();
  }, [slideAnim, backdropAnim]);

  useEffect(() => {
    const sub = BackHandler.addEventListener('hardwareBackPress', () => {
      close();
      return true;
    });
    return () => sub.remove();
  }, [close]);

  const toggleToken = useCallback((index: number) => {
    setSelected((prev) => {
      const next = new Set(prev);
      if (next.has(index)) {
        next.delete(index);
      } else {
        next.add(index);
      }
      return next;
    });
  }, []);

  const selectAll = useCallback(() => {
    const all = new Set<number>();
    tokens.forEach((t, i) => {
      if (!/^\s+$/.test(t)) {
        all.add(i);
      }
    });
    setSelected(all);
  }, [tokens]);

  const clearSelection = useCallback(() => {
    setSelected(new Set());
  }, []);

  const handleCopy = useCallback(async () => {
    const selectedText = tokens.filter((_, i) => selected.has(i)).join('');
    if (selectedText) {
      await Clipboard.setStringAsync(selectedText);
      ToastAndroid.show('已复制', ToastAndroid.SHORT);
    }
  }, [tokens, selected]);

  return (
    <View style={styles.container}>
      {/* 半透明背景 */}
      <Pressable style={StyleSheet.absoluteFill} onPress={close}>
        <Animated.View
          style={[
            StyleSheet.absoluteFill,
            { backgroundColor: theme.colors.backdrop, opacity: backdropAnim },
          ]}
        />
      </Pressable>

      {/* 底部弹出面板 */}
      <Animated.View
        style={[
          styles.sheet,
          {
            backgroundColor: theme.colors.surface,
            transform: [{ translateY: slideAnim }],
          },
        ]}
      >
        {/* 拖动指示条 */}
        <View style={styles.handleContainer}>
          <View style={[styles.handle, { backgroundColor: theme.colors.border }]} />
        </View>

        {/* 标题栏 */}
        <View style={[styles.header, { borderBottomColor: theme.colors.divider }]}>
          <Text style={[styles.headerTitle, { color: theme.colors.text }]}>分词选择</Text>
          <View style={styles.headerRight}>
            <Text style={[styles.splitAllLabel, { color: theme.colors.textSecondary }]}>逐字</Text>
            <Switch
              value={splitAll}
              onValueChange={setSplitAll}
              trackColor={{
                false: theme.colors.border,
                true: theme.colors.primary + '80',
              }}
              thumbColor={splitAll ? theme.colors.primary : theme.colors.surface}
              style={styles.switchStyle}
            />
            <TouchableOpacity onPress={close} style={styles.closeButton}>
              <Text style={[styles.closeButtonText, { color: theme.colors.primary }]}>关闭</Text>
            </TouchableOpacity>
          </View>
        </View>

        {/* 选中预览 */}
        <ScrollView
          style={[styles.previewContainer, { backgroundColor: theme.colors.background }]}
          nestedScrollEnabled
        >
          <Text style={[styles.previewLabel, { color: theme.colors.textSecondary }]}>
            已选 {selected.size} 个词
          </Text>
          <Text style={[styles.previewText, { color: theme.colors.text }]}>
            {selected.size > 0
              ? tokens.filter((_, i) => selected.has(i)).join('')
              : '点击下方文字选择'}
          </Text>
        </ScrollView>

        {/* 分词区域 */}
        <ScrollView style={styles.scrollView} contentContainerStyle={styles.tokensContainer}>
          <View style={styles.tokensWrap}>
            {tokens.map((token, index) => {
              const isWhitespace = /^\s+$/.test(token);
              if (isWhitespace) {
                return (
                  <Text key={index} style={styles.whitespace}>
                    {token}
                  </Text>
                );
              }
              const isSelected = selected.has(index);
              return (
                <TouchableOpacity
                  key={index}
                  onPress={() => toggleToken(index)}
                  style={[
                    styles.token,
                    {
                      backgroundColor: isSelected ? theme.colors.primary : theme.colors.surface,
                      borderColor: isSelected ? theme.colors.primary : theme.colors.border,
                    },
                  ]}
                >
                  <Text
                    style={[
                      styles.tokenText,
                      { color: isSelected ? theme.colors.white : theme.colors.text },
                    ]}
                  >
                    {token}
                  </Text>
                </TouchableOpacity>
              );
            })}
          </View>
        </ScrollView>

        {/* 底部操作栏 */}
        <View
          style={[
            styles.bottomBar,
            { backgroundColor: theme.colors.surface, borderTopColor: theme.colors.divider },
          ]}
        >
          <TouchableOpacity
            style={[styles.bottomButton, { borderColor: theme.colors.border }]}
            onPress={selectAll}
          >
            <Text style={[styles.bottomButtonText, { color: theme.colors.primary }]}>全选</Text>
          </TouchableOpacity>
          <TouchableOpacity
            style={[styles.bottomButton, { borderColor: theme.colors.border }]}
            onPress={clearSelection}
          >
            <Text style={[styles.bottomButtonText, { color: theme.colors.primary }]}>清除</Text>
          </TouchableOpacity>
          <TouchableOpacity
            style={[
              styles.bottomButton,
              styles.copyButton,
              {
                backgroundColor: selected.size > 0 ? theme.colors.primary : theme.colors.disabled,
              },
            ]}
            onPress={handleCopy}
            disabled={selected.size === 0}
          >
            <Text
              style={[
                styles.bottomButtonText,
                { color: selected.size > 0 ? theme.colors.white : theme.colors.textDisabled },
              ]}
            >
              复制
            </Text>
          </TouchableOpacity>
        </View>
      </Animated.View>
    </View>
  );
};

const styles = StyleSheet.create({
  container: {
    flex: 1,
    justifyContent: 'flex-end',
  },
  sheet: {
    height: SHEET_HEIGHT,
    borderTopLeftRadius: 20,
    borderTopRightRadius: 20,
    overflow: 'hidden',
  },
  handleContainer: {
    alignItems: 'center',
    paddingTop: 10,
    paddingBottom: 4,
  },
  handle: {
    width: 36,
    height: 4,
    borderRadius: 2,
  },
  header: {
    flexDirection: 'row',
    alignItems: 'center',
    justifyContent: 'space-between',
    paddingHorizontal: 16,
    paddingVertical: 12,
    borderBottomWidth: 1,
  },
  headerTitle: {
    fontSize: 18,
    fontWeight: '600',
  },
  headerRight: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: 6,
  },
  splitAllLabel: {
    fontSize: 14,
  },
  switchStyle: {
    transform: [{ scaleX: 0.8 }, { scaleY: 0.8 }],
  },
  closeButton: {
    paddingHorizontal: 8,
    paddingVertical: 4,
  },
  closeButtonText: {
    fontSize: 16,
    fontWeight: '500',
  },
  previewContainer: {
    maxHeight: 100,
    padding: 12,
    marginHorizontal: 12,
    marginTop: 10,
    borderRadius: 10,
  },
  previewLabel: {
    fontSize: 13,
    marginBottom: 4,
  },
  previewText: {
    fontSize: 15,
    lineHeight: 22,
  },
  scrollView: {
    flex: 1,
  },
  tokensContainer: {
    padding: 12,
  },
  tokensWrap: {
    flexDirection: 'row',
    flexWrap: 'wrap',
    alignItems: 'center',
  },
  whitespace: {
    width: 6,
  },
  token: {
    paddingHorizontal: 10,
    paddingVertical: 6,
    borderRadius: 8,
    borderWidth: 1,
    margin: 3,
  },
  tokenText: {
    fontSize: 16,
    lineHeight: 22,
  },
  bottomBar: {
    flexDirection: 'row',
    alignItems: 'center',
    justifyContent: 'center',
    paddingHorizontal: 16,
    paddingVertical: 12,
    borderTopWidth: 1,
    gap: 12,
  },
  bottomButton: {
    flex: 1,
    alignItems: 'center',
    paddingVertical: 12,
    borderRadius: 10,
    borderWidth: 1,
  },
  bottomButtonText: {
    fontSize: 16,
    fontWeight: '600',
  },
  copyButton: {
    borderWidth: 0,
  },
});
