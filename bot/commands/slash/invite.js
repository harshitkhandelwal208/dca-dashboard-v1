const { SlashCommandBuilder } = require("discord.js");
const { sendInviteToApplicant } = require("../../utils/recruitmentManager");

module.exports = {
    data: new SlashCommandBuilder()
        .setName("invite")
        .setDescription("Send the ticket applicant a single-use destination server invite."),

    async execute(interaction) {
        await interaction.deferReply({ ephemeral: true });
        await sendInviteToApplicant(interaction);
    }
};
