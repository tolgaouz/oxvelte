import svelte from "eslint-plugin-svelte";

export default [
  ...svelte.configs["flat/recommended"],
  {
    files: ["**/*.svelte"],
    languageOptions: {
      parserOptions: {
        parser: null
      }
    }
  }
];
