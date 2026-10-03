// Commit messages must follow Conventional Commits (feat:, fix:, chore:, ...);
// semantic-release reads them to pick the next version.
export default {
  extends: ['@commitlint/config-conventional'],
};
