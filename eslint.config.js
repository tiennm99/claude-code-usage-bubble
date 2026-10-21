import js from '@eslint/js';
import jsdoc from 'eslint-plugin-jsdoc';
import globals from 'globals';

export default [
  {
    ignores: ['target/', 'gen/', 'node_modules/'],
  },
  js.configs.recommended,
  jsdoc.configs['flat/recommended-error'],
  {
    files: ['ui/**/*.js'],
    languageOptions: {
      globals: {
        ...globals.browser,
        __TAURI__: 'readonly',
      },
    },
  },
  {
    files: ['tests/**/*.js', 'scripts/**/*.js', 'eslint.config.js'],
    languageOptions: {
      globals: globals.node,
    },
  },
];
