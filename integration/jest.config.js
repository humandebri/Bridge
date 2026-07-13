module.exports = {
  rootDir: "..",
  testMatch: ["<rootDir>/integration/**/*.spec.ts"],
  transform: { "^.+\\.tsx?$": ["@swc/jest"] },
  testTimeout: 120000,
};
