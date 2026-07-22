const { SlashCommandBuilder, PermissionFlagsBits } = require("discord.js");
const { clearWarnings } = require("../../utils/warningStore");

module.exports = {
  data: new SlashCommandBuilder()
    .setName("clearwarns")
    .setDescription("Clears all warnings for a user.")
    .addUserOption(option =>
      option.setName("user")
        .setDescription("User whose warnings will be cleared")
        .setRequired(true))
    .setDefaultMemberPermissions(PermissionFlagsBits.ManageMessages),

  async execute(interaction) {
    const user = interaction.options.getUser("user");

    try {
      const removed = await clearWarnings(user.id, interaction.guild.id);

      if (removed === 0) {
        return interaction.reply({ content: `❌ **${user.tag}** has no warnings to clear.`, ephemeral: true });
      }

      return interaction.reply({ content: `✅ Cleared all warnings for **${user.tag}**.`, ephemeral: false });
    } catch (error) {
      console.error(error);
      return interaction.reply({ content: "❌ An error occurred while clearing warnings.", ephemeral: true });
    }
  },
};
