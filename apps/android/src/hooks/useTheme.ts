/**
 * useTheme Hook
 * 用于访问和操作主题
 */

import { useContext } from 'react';
import { ThemeContext } from '@/contexts/ThemeContext';

export const useTheme = () => {
  const context = useContext(ThemeContext);

  if (!context) {
    throw new Error('useTheme must be used within ThemeProvider');
  }

  return context;
};
