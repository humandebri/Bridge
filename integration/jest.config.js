module.exports = {
  rootDir: ".",
  testMatch: ["<rootDir>/**/*.spec.ts"],
  transform: { "^.+\\.tsx?$": ["@swc/jest"] },
  testTimeout: 120000,
};
