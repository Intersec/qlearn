import js from '@eslint/js';
import pluginVue from 'eslint-plugin-vue';
import { defineConfigWithVueTs, vueTsConfigs } from '@vue/eslint-config-typescript';
import skipFormatting from '@vue/eslint-config-prettier/skip-formatting';

export default defineConfigWithVueTs(
  { name: 'app/files', files: ['**/*.{ts,mts,tsx,vue}'] },
  { name: 'app/ignores', ignores: ['dist/**', 'coverage/**'] },
  js.configs.recommended,
  pluginVue.configs['flat/recommended'],
  vueTsConfigs.recommended,
  skipFormatting,
  {
    name: 'app/rules',
    rules: {
      // Braces on every if. A one-line body grows a second line one day.
      curly: ['error', 'all'],
      'no-console': ['error', { allow: ['warn', 'error'] }],
    },
  },
  {
    // A script whose whole job is to say where the pictures went. This
    // block is last on purpose: in a flat configuration the later one wins.
    name: 'app/scripts',
    files: ['e2e/shots.ts'],
    rules: { 'no-console': 'off' },
  },
);
