import { create } from 'zustand';
import {
  TransferTask,
  TransferType,
  TransferTaskStatus,
  getHistoryTransferQueue,
} from '@/services/HistoryTransferQueue';
import { useErrorStore } from './errorStore';
import { useMessageStore } from './messageStore';

interface TransferQueueState {
  tasks: TransferTask[];
  pendingCount: number;
  activeCount: number;
  hasTasks: boolean;

  subscribe: () => () => void;
  getTasks: () => TransferTask[];
  getPendingTasks: () => TransferTask[];
  getActiveTasks: () => TransferTask[];
  getTasksByStatus: (status: TransferTaskStatus) => TransferTask[];
  getTasksByType: (type: TransferType) => TransferTask[];
}

export const useTransferQueueStore = create<TransferQueueState>((set, get) => ({
  tasks: [],
  pendingCount: 0,
  activeCount: 0,
  hasTasks: false,

  subscribe: () => {
    const queue = getHistoryTransferQueue();

    const handleTaskStatusChanged = (task: TransferTask) => {
      const currentTasks = get().tasks;
      const existingIndex = currentTasks.findIndex(
        (t) => t.profileId === task.profileId && t.type === task.type
      );

      let newTasks: TransferTask[];
      if (existingIndex >= 0) {
        newTasks = [...currentTasks];
        newTasks[existingIndex] = task;
      } else {
        newTasks = [...currentTasks, task];
      }

      newTasks = newTasks.filter((t) => t.status !== 'completed' && t.status !== 'cancelled');

      const pendingCount = newTasks.filter(
        (t) => t.status === 'pending' || t.status === 'waitForRetry'
      ).length;
      const activeCount = newTasks.filter((t) => t.status === 'running').length;

      set({
        tasks: newTasks,
        pendingCount,
        activeCount,
        hasTasks: newTasks.length > 0,
      });

      if (task.status === 'failed' && task.errorMessage && !task.userCancelled) {
        const operationName = task.type === 'download' ? '下载' : '上传';
        useErrorStore.getState().showNetworkError(operationName, task.errorMessage);
        useMessageStore
          .getState()
          .showMessage(`${operationName}失败: ${task.errorMessage}`, 'error');
      }
    };

    queue.onTaskStatusChanged(handleTaskStatusChanged);

    const activeTasks = queue.getActiveTasks();
    const initialTasks = activeTasks.filter(
      (t) => t.status !== 'completed' && t.status !== 'cancelled'
    );

    set({
      tasks: initialTasks,
      pendingCount: initialTasks.filter(
        (t) => t.status === 'pending' || t.status === 'waitForRetry'
      ).length,
      activeCount: initialTasks.filter((t) => t.status === 'running').length,
      hasTasks: initialTasks.length > 0,
    });

    return () => {
      queue.offTaskStatusChanged(handleTaskStatusChanged);
    };
  },

  getTasks: () => get().tasks,

  getPendingTasks: () =>
    get().tasks.filter((t) => t.status === 'pending' || t.status === 'waitForRetry'),

  getActiveTasks: () => get().tasks.filter((t) => t.status === 'running'),

  getTasksByStatus: (status: TransferTaskStatus) => get().tasks.filter((t) => t.status === status),

  getTasksByType: (type: TransferType) => get().tasks.filter((t) => t.type === type),
}));
